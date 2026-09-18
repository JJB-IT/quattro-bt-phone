//! Persistent settings in `$XDG_CONFIG_HOME/quattro-bt-phone/config.toml`.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Bluetooth address of the phone to use. Chosen in the setup screen if unset.
    pub phone: Option<String>,
    /// Record every call automatically. Off by default: recording laws differ per country.
    pub auto_record: bool,
    /// Where recordings go. Defaults to `$XDG_MUSIC_DIR/Calls` (or `~/Music/Calls`).
    pub recordings_dir: Option<PathBuf>,
    /// Show desktop notifications for incoming and missed calls.
    pub notifications: Option<bool>,
    /// PipeWire `node.name` of the speakers or headset for calls. Unset: the system default.
    pub audio_output: Option<String>,
    /// PipeWire `node.name` of the microphone for calls. Unset: the system default.
    pub audio_input: Option<String>,
    /// Play a tone for each dialled digit.
    pub keypad_sounds: bool,
    /// Ringing tone while an outgoing call rings: `europe`, `uk`, `north_america` or `off`.
    pub ringback: qbp_proto::RingbackStyle,
    /// With `ringback = "custom"`: a file name in `~/.config/quattro-bt-phone/ringtones/`.
    pub ringback_file: Option<String>,
}

impl Config {
    pub fn default_path() -> PathBuf {
        xdg_dir("XDG_CONFIG_HOME", ".config").join("quattro-bt-phone/config.toml")
    }

    /// Missing file → defaults. A malformed file is an error rather than being silently replaced.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(s) => toml::from_str(&s).with_context(|| format!("parsing {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, toml::to_string_pretty(self)?)?;
        std::fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))
    }

    pub fn notifications(&self) -> bool {
        self.notifications.unwrap_or(true)
    }
}

pub fn xdg_dir(var: &str, fallback_under_home: &str) -> PathBuf {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(fallback_under_home))
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_defaults() {
        let dir = std::env::temp_dir().join(format!("qbp-config-{}", std::process::id()));
        let path = dir.join("config.toml");
        assert_eq!(Config::load(&path).unwrap(), Config::default());

        let c = Config { phone: Some("AA:BB:CC:DD:EE:FF".into()), auto_record: true, ..Default::default() };
        c.save(&path).unwrap();
        assert_eq!(Config::load(&path).unwrap(), c);
        assert!(c.notifications());

        std::fs::write(&path, "bogus = 1").unwrap();
        assert!(Config::load(&path).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
