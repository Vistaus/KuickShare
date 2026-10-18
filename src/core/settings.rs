//! Persistent user settings, stored as JSON under the XDG config dir.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::allowed::AllowedDevice;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// `None` falls back to the machine hostname.
    pub device_name: Option<String>,
    /// `None` falls back to the library default (the XDG download dir).
    pub download_dir: Option<PathBuf>,
    /// `None` picks a random port at every start.
    pub static_port: Option<u32>,
    pub visible: bool,
    pub hide_when_dnd: bool,
    /// Devices whose incoming transfers are accepted without asking.
    pub allowed_devices: Vec<AllowedDevice>,
    pub history_enabled: bool,
    /// History entries older than this many days are deleted (0 keeps them).
    pub history_keep_days: u32,
    /// Without the tray icon, closing the window quits the app.
    pub tray_icon: bool,
    pub dolphin_integration: bool,
    pub trusted_devices: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            device_name: None,
            download_dir: None,
            static_port: None,
            visible: true,
            hide_when_dnd: true,
            allowed_devices: Vec::new(),
            history_enabled: true,
            history_keep_days: 0,
            tray_icon: true,
            dolphin_integration: true,
            trusted_devices: true,
        }
    }
}

/// Moves an unreadable file to `<name>.bak` so the next save does not destroy it.
pub fn keep_aside(path: &Path) {
    let bak = path.with_extension("json.bak");
    if let Err(e) = fs::rename(path, &bak) {
        tracing::warn!("cannot keep {} aside: {e}", path.display());
    }
}

pub fn default_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("kuickshare").join("settings.json"))
}

/// Hostname used when no device name is configured.
pub fn default_device_name() -> String {
    fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|s| s.trim().to_owned())
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Unknown device".to_owned())
}

impl Settings {
    /// A missing or unreadable file yields the defaults; the problem is logged, not fatal.
    pub fn load(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                tracing::warn!("ignoring invalid settings file {}: {e}", path.display());
                keep_aside(path);
                Self::default()
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => {
                tracing::warn!("cannot read settings file {}: {e}", path.display());
                Self::default()
            }
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let dir = path.parent().context("settings path has no parent")?;
        fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(self)?)
            .with_context(|| format!("cannot write {}", tmp.display()))?;
        fs::rename(&tmp, path).with_context(|| format!("cannot replace {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kuickshare-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn save_then_load_round_trips() {
        let path = temp_dir("roundtrip").join("settings.json");
        let settings = Settings {
            device_name: Some("desk".into()),
            download_dir: Some("/tmp/dl".into()),
            static_port: Some(40000),
            visible: false,
            hide_when_dnd: false,
            allowed_devices: vec![AllowedDevice {
                name: "phone".into(),
                mac: "aa:bb:cc:dd:ee:01".into(),
            }],
            history_enabled: false,
            history_keep_days: 30,
            tray_icon: false,
            dolphin_integration: false,
            trusted_devices: false,
        };
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), settings);
    }

    #[test]
    fn missing_or_invalid_file_gives_defaults() {
        let dir = temp_dir("invalid");
        assert_eq!(
            Settings::load(&dir.join("absent.json")),
            Settings::default()
        );

        fs::create_dir_all(&dir).unwrap();
        let bad = dir.join("bad.json");
        fs::write(&bad, "{not json").unwrap();
        assert_eq!(Settings::load(&bad), Settings::default());
    }

    #[test]
    fn partial_file_keeps_defaults_for_missing_keys() {
        let dir = temp_dir("partial");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(&path, r#"{"device_name":"x"}"#).unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded.device_name.as_deref(), Some("x"));
        assert!(loaded.visible);
        assert!(loaded.hide_when_dnd);
    }
}
