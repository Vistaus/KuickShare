//! The 0xFEF3 receiver advertisement (BLE service data, also served from GATT slot 0).
//!
//! Layout (reverse-engineered from a Pixel 9 Pro capture, see docs):
//! `48 | fc9f5e | u32 BE len | DATA | token(2) | mask | [psm(2)] | [len | rx_instant]`
//! with `DATA = 23 | fc9f5e | endpoint_id(4) | info_len | endpoint_info | mac(6) | 00 00`.

use super::socket::SERVICE_HASH;

/// 16-bit service UUID of the receiver advertisement and the GATT service.
pub const SERVICE_UUID: u16 = 0xFEF3;
pub const DEVICE_TYPE_LAPTOP: u8 = 3;
/// Display names are cut at this many bytes so the advertisement stays well under 251 bytes.
pub const MAX_NAME_LEN: usize = 100;

const HEADER: u8 = 0x48;
const DATA_VERSION_PCP: u8 = 0x23;
const MASK_PSM: u8 = 0x01;
const MASK_RX_INSTANT: u8 = 0x02;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiverAdvertisement {
    /// Must equal the endpoint id used in the mDNS instance name.
    pub endpoint_id: [u8; 4],
    pub device_type: u8,
    /// Random per run; the same 16 bytes as in the mDNS endpoint info.
    pub identity: [u8; 16],
    pub name: String,
    /// All zeroes by default: a wrong MAC makes the phone try Bluetooth Classic first.
    pub bt_mac: [u8; 6],
    /// L2CAP PSM to advertise. `None` clears the mask bit (UNVERIFIED on a phone);
    /// the proven capture advertised `Some(0x0082)`.
    pub psm: Option<u16>,
    /// Random device token (Nearby derives it from a random number); carries no identity.
    pub token: [u8; 2],
    /// The "RxInstantConnectionAdv" field (at most 255 bytes). `None` leaves it out: the
    /// bytes seen in a Pixel capture are opaque and possibly tied to that device's account,
    /// so they are not reused. A Pixel 7 listed the receiver without them (one hardware run).
    pub rx_instant: Option<Vec<u8>>,
}

impl ReceiverAdvertisement {
    pub fn new(endpoint_id: [u8; 4], device_type: u8, identity: [u8; 16], name: &str) -> Self {
        Self {
            endpoint_id,
            device_type,
            identity,
            name: truncate_name(name).to_owned(),
            bt_mac: [0; 6],
            psm: None,
            token: rand::random(),
            rx_instant: None,
        }
    }

    /// Nearby Share application advertisement (same layout as the mDNS `n` record).
    pub fn endpoint_info(&self) -> Vec<u8> {
        let name = truncate_name(&self.name).as_bytes();
        let mut info = Vec::with_capacity(18 + name.len());
        info.push((1 << 5) | ((self.device_type & 0x7) << 1));
        info.extend_from_slice(&self.identity);
        info.push(name.len() as u8);
        info.extend_from_slice(name);
        info
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let info = self.endpoint_info();
        let mut data = vec![DATA_VERSION_PCP];
        data.extend_from_slice(&SERVICE_HASH);
        data.extend_from_slice(&self.endpoint_id);
        data.push(info.len() as u8);
        data.extend_from_slice(&info);
        data.extend_from_slice(&self.bt_mac);
        data.extend_from_slice(&[0, 0]);

        let mut out = vec![HEADER];
        out.extend_from_slice(&SERVICE_HASH);
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(&data);
        out.extend_from_slice(&self.token);
        let rx_instant = self.rx_instant.as_deref().filter(|d| d.len() <= 255);
        out.push(
            if self.psm.is_some() { MASK_PSM } else { 0 }
                | if rx_instant.is_some() {
                    MASK_RX_INSTANT
                } else {
                    0
                },
        );
        if let Some(psm) = self.psm {
            out.extend_from_slice(&psm.to_be_bytes());
        }
        if let Some(data) = rx_instant {
            out.push(data.len() as u8);
            out.extend_from_slice(data);
        }
        out
    }

    /// Parses an advertisement.
    #[cfg(test)]
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let (&header, rest) = bytes.split_first()?;
        if header != HEADER || rest.get(..3)? != SERVICE_HASH {
            return None;
        }
        let len = u32::from_be_bytes(rest.get(3..7)?.try_into().ok()?) as usize;
        let data = rest.get(7..7usize.checked_add(len)?)?;
        let tail = &rest[7 + len..];

        if *data.first()? != DATA_VERSION_PCP || data.get(1..4)? != SERVICE_HASH {
            return None;
        }
        let endpoint_id: [u8; 4] = data.get(4..8)?.try_into().ok()?;
        let info_len = *data.get(8)? as usize;
        let info = data.get(9..9 + info_len)?;
        let bt_mac: [u8; 6] = data.get(9 + info_len..15 + info_len)?.try_into().ok()?;

        let device_type = (info.first()? >> 1) & 0x7;
        let identity: [u8; 16] = info.get(1..17)?.try_into().ok()?;
        let name_len = *info.get(17)? as usize;
        let name = std::str::from_utf8(info.get(18..18 + name_len)?).ok()?;

        let mask = *tail.get(2)?;
        let token: [u8; 2] = tail.get(..2)?.try_into().ok()?;
        let mut at = 3;
        let psm = if mask & MASK_PSM != 0 {
            at += 2;
            Some(u16::from_be_bytes(tail.get(at - 2..at)?.try_into().ok()?))
        } else {
            None
        };
        let rx_instant = if mask & MASK_RX_INSTANT != 0 {
            let len = *tail.get(at)? as usize;
            Some(tail.get(at + 1..at + 1 + len)?.to_vec())
        } else {
            None
        };

        Some(Self {
            endpoint_id,
            device_type,
            identity,
            name: name.to_owned(),
            bt_mac,
            psm,
            token,
            rx_instant,
        })
    }
}

fn truncate_name(name: &str) -> &str {
    if name.len() <= MAX_NAME_LEN {
        return name;
    }
    let mut end = MAX_NAME_LEN;
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    &name[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 132-byte service data captured from a Pixel 9 Pro, with endpoint id and name replaced
    /// by "PKT1" / "Packet Linux Test 01" (quickshare-direct examples/rx_advert.rs).
    const CAPTURE: [u8; 132] = [
        0x48, 0xfc, 0x9f, 0x5e, 0x00, 0x00, 0x00, 0x37, 0x23, 0xfc, 0x9f, 0x5e, 0x50, 0x4b, 0x54,
        0x31, 0x26, 0x22, 0x4a, 0x22, 0x71, 0x16, 0x9c, 0x15, 0x99, 0xa2, 0x44, 0xaf, 0x44, 0xb0,
        0x17, 0x9c, 0x0f, 0x23, 0x14, 0x50, 0x61, 0x63, 0x6b, 0x65, 0x74, 0x20, 0x4c, 0x69, 0x6e,
        0x75, 0x78, 0x20, 0x54, 0x65, 0x73, 0x74, 0x20, 0x30, 0x31, 0xfc, 0x41, 0x16, 0xb6, 0x17,
        0x20, 0x00, 0x00, 0x62, 0xf1, 0x03, 0x00, 0x82, 0x3f, 0xa0, 0x17, 0xfd, 0xf1, 0x70, 0x59,
        0x6e, 0x1e, 0xd3, 0x4d, 0xe0, 0x92, 0x56, 0x4d, 0x66, 0xd4, 0x29, 0x0f, 0x0f, 0x8f, 0x15,
        0x05, 0x34, 0x7b, 0x13, 0x23, 0x01, 0xea, 0x7f, 0x92, 0xa8, 0xd8, 0xd4, 0x61, 0x84, 0x15,
        0x05, 0x3f, 0x00, 0x00, 0x84, 0x15, 0x06, 0x2d, 0x00, 0x00, 0x84, 0x15, 0x04, 0x7f, 0x1f,
        0x00, 0x84, 0x15, 0x07, 0x2d, 0x1f, 0x00, 0x83, 0x15, 0x01, 0x15, 0x7c,
    ];

    fn captured() -> ReceiverAdvertisement {
        ReceiverAdvertisement {
            endpoint_id: *b"PKT1",
            device_type: 1,
            identity: [
                0x4a, 0x22, 0x71, 0x16, 0x9c, 0x15, 0x99, 0xa2, 0x44, 0xaf, 0x44, 0xb0, 0x17, 0x9c,
                0x0f, 0x23,
            ],
            name: "Packet Linux Test 01".into(),
            bt_mac: [0xfc, 0x41, 0x16, 0xb6, 0x17, 0x20],
            psm: Some(0x0082),
            token: [0x62, 0xf1],
            rx_instant: Some(CAPTURE[CAPTURE.len() - 63..].to_vec()),
        }
    }

    #[test]
    fn builds_the_captured_bytes() {
        assert_eq!(captured().to_bytes(), CAPTURE);
    }

    #[test]
    fn parses_the_captured_bytes() {
        assert_eq!(ReceiverAdvertisement::parse(&CAPTURE), Some(captured()));
    }

    #[test]
    fn laptop_without_psm_layout() {
        let mut adv = ReceiverAdvertisement::new(*b"ABCD", DEVICE_TYPE_LAPTOP, [7; 16], "Desk");
        adv.bt_mac = [0; 6];
        adv.token = [0x62, 0xf1];
        let b = adv.to_bytes();
        assert_eq!(&b[..8], [0x48, 0xfc, 0x9f, 0x5e, 0, 0, 0, 0x27]);
        assert_eq!(&b[8..16], [0x23, 0xfc, 0x9f, 0x5e, b'A', b'B', b'C', b'D']);
        // info length 1 + 16 + 1 + 4, header byte 0x26 (visible laptop)
        assert_eq!(&b[16..18], [22, 0x26]);
        assert_eq!(&b[18..34], [7; 16]);
        assert_eq!(&b[34..39], [4, b'D', b'e', b's', b'k']);
        assert_eq!(&b[39..47], [0; 8]);
        assert_eq!(&b[47..49], [0x62, 0xf1]);
        // No PSM and none of the captured opaque field: the mask is empty and nothing follows.
        assert_eq!(&b[49..], [0x00]);
        assert_eq!(ReceiverAdvertisement::parse(&b), Some(adv));
    }

    #[test]
    fn psm_is_big_endian_after_the_mask() {
        let mut adv = ReceiverAdvertisement::new(*b"ABCD", 3, [0; 16], "x");
        adv.psm = Some(0x00a1);
        let b = adv.to_bytes();
        let off = 8 + b[7] as usize;
        assert_eq!(&b[off + 2..off + 5], [0x01, 0x00, 0xa1]);
        assert_eq!(ReceiverAdvertisement::parse(&b).unwrap().psm, Some(0xa1));
    }

    #[test]
    fn the_token_is_random_per_advertisement() {
        let tokens: std::collections::HashSet<_> = (0..32)
            .map(|_| ReceiverAdvertisement::new(*b"ABCD", 3, [0; 16], "x").token)
            .collect();
        assert!(tokens.len() > 1);
    }

    #[test]
    fn long_names_are_cut_on_a_char_boundary() {
        let adv = ReceiverAdvertisement::new(*b"ABCD", 3, [0; 16], &"è".repeat(80));
        assert_eq!(adv.name.len(), 100);
        let parsed = ReceiverAdvertisement::parse(&adv.to_bytes()).unwrap();
        assert_eq!(parsed.name, adv.name);
        assert!(adv.to_bytes().len() < 251);
    }

    #[test]
    fn rejects_garbage_and_truncation() {
        assert_eq!(ReceiverAdvertisement::parse(&[]), None);
        assert_eq!(
            ReceiverAdvertisement::parse(&[0x49, 0xfc, 0x9f, 0x5e]),
            None
        );
        for n in 0..CAPTURE.len() - 1 {
            // Cuts inside the fixed part must never panic; most return None.
            let _ = ReceiverAdvertisement::parse(&CAPTURE[..n]);
        }
        assert_eq!(ReceiverAdvertisement::parse(&CAPTURE[..62]), None);
    }
}
