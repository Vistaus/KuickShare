//! Devices whose incoming transfers are accepted without asking.
//!
//! A device is identified by its advertised name plus the MAC the kernel has for its IP (ARP table).
//! The name alone can be copied by anyone nearby; a device with another MAC is asked about again.

use std::{
    fs,
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllowedDevice {
    pub name: String,
    /// Lowercase, colon separated.
    pub mac: String,
}

pub type Allowed = Arc<Mutex<Vec<AllowedDevice>>>;

pub fn is_allowed(list: &[AllowedDevice], name: &str, mac: &str) -> bool {
    !name.is_empty() && list.iter().any(|d| d.name == name && d.mac == mac)
}

/// The MAC is looked up lazily, only when the option is on and the name is non-empty.
pub fn trusted_sender(
    list: &[AllowedDevice],
    enabled: bool,
    name: &str,
    mac: impl FnOnce() -> Option<String>,
) -> bool {
    enabled && !name.is_empty() && mac().is_some_and(|mac| is_allowed(list, name, &mac))
}

/// MAC address of a device on the local network, if the kernel knows it.
pub fn mac_for_ip(ip: &str) -> Option<String> {
    let table = fs::read_to_string("/proc/net/arp").ok()?;
    parse_arp(&table, ip)
}

fn parse_arp(table: &str, ip: &str) -> Option<String> {
    // Columns: IP address, HW type, Flags, HW address, Mask, Device.
    table.lines().skip(1).find_map(|line| {
        let mut cols = line.split_whitespace();
        let (entry_ip, _, flags, mac) = (cols.next()?, cols.next()?, cols.next()?, cols.next()?);
        // Flags 0x0 marks an incomplete entry.
        (entry_ip == ip && flags != "0x0" && mac != "00:00:00:00:00:00")
            .then(|| mac.to_ascii_lowercase())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARP: &str = "\
IP address       HW type     Flags       HW address            Mask     Device
192.168.1.20     0x1         0x2         AA:BB:CC:DD:EE:01     *        wlan0
192.168.1.21     0x1         0x0         00:00:00:00:00:00     *        wlan0
";

    #[test]
    fn finds_complete_entries_only() {
        assert_eq!(
            parse_arp(ARP, "192.168.1.20").as_deref(),
            Some("aa:bb:cc:dd:ee:01")
        );
        assert_eq!(parse_arp(ARP, "192.168.1.21"), None);
        assert_eq!(parse_arp(ARP, "192.168.1.22"), None);
    }

    #[test]
    fn a_trusted_sender_needs_the_option_on_and_a_matching_name_and_mac() {
        let list = vec![AllowedDevice {
            name: "Pixel".into(),
            mac: "aa:bb:cc:dd:ee:01".into(),
        }];
        let known = || Some("aa:bb:cc:dd:ee:01".to_owned());
        assert!(trusted_sender(&list, true, "Pixel", known));
        assert!(!trusted_sender(&list, false, "Pixel", known));
        assert!(!trusted_sender(&list, true, "Other", known));
        assert!(!trusted_sender(&list, true, "Pixel", || Some(
            "aa:bb:cc:dd:ee:02".into()
        )));
        // The kernel does not know the address (another network, IPv6): not trusted.
        assert!(!trusted_sender(&list, true, "Pixel", || None));
        // The MAC is not even looked up when the option is off.
        assert!(!trusted_sender(&list, false, "Pixel", || panic!(
            "looked up"
        )));
    }

    #[test]
    fn needs_both_name_and_mac() {
        let list = vec![AllowedDevice {
            name: "Pixel".into(),
            mac: "aa:bb:cc:dd:ee:01".into(),
        }];
        assert!(is_allowed(&list, "Pixel", "aa:bb:cc:dd:ee:01"));
        assert!(!is_allowed(&list, "Pixel", "aa:bb:cc:dd:ee:02"));
        assert!(!is_allowed(&list, "Other", "aa:bb:cc:dd:ee:01"));
        assert!(!is_allowed(&list, "", "aa:bb:cc:dd:ee:01"));
    }
}
