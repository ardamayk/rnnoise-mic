//! Tray settings kept between runs, in a small `key=value` file:
//! `%APPDATA%\rnnoise-mic\settings.txt` on Windows,
//! `$XDG_CONFIG_HOME/rnnoise-mic/settings.txt` (or `~/.config/...`) on Linux.

use std::path::PathBuf;

/// "Mute when not speaking" presets: RNNoise voice probability below which audio is muted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gate {
    Off,
    Low,
    Medium,
    High,
}

impl Gate {
    pub const ALL: [Gate; 4] = [Gate::Off, Gate::Low, Gate::Medium, Gate::High];

    pub fn threshold(self) -> f32 {
        match self {
            Gate::Off => 0.0,
            Gate::Low => 0.4,
            Gate::Medium => 0.6,
            Gate::High => 0.8,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Gate::Off => "Off",
            Gate::Low => "Low - keeps soft speech",
            Gate::Medium => "Medium (recommended)",
            Gate::High => "High - blocks most typing",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Gate::Off => "off",
            Gate::Low => "low",
            Gate::Medium => "medium",
            Gate::High => "high",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Settings {
    pub suppression: bool,
    pub gate: Gate,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { suppression: true, gate: Gate::Off }
    }
}

fn path() -> Option<PathBuf> {
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    base.map(|b| b.join("rnnoise-mic").join("settings.txt"))
}

impl Settings {
    pub fn parse(text: &str) -> Self {
        let mut s = Settings::default();
        for line in text.lines() {
            match line.split_once('=').map(|(k, v)| (k.trim(), v.trim())) {
                Some(("suppression", v)) => s.suppression = v != "off",
                Some(("gate", v)) => s.gate = Gate::ALL.into_iter().find(|g| g.key() == v).unwrap_or(Gate::Off),
                _ => {}
            }
        }
        s
    }

    pub fn to_text(self) -> String {
        format!(
            "suppression={}\ngate={}\n",
            if self.suppression { "on" } else { "off" },
            self.gate.key()
        )
    }

    /// Missing or unreadable file means defaults.
    pub fn load() -> Self {
        path().and_then(|p| std::fs::read_to_string(p).ok()).map(|t| Self::parse(&t)).unwrap_or_default()
    }

    pub fn save(self) -> std::io::Result<()> {
        let p = path().ok_or_else(|| std::io::Error::other("no config directory"))?;
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(p, self.to_text())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let s = Settings { suppression: false, gate: Gate::High };
        assert_eq!(Settings::parse(&s.to_text()), s);
    }

    #[test]
    fn garbage_falls_back_to_defaults() {
        assert_eq!(Settings::parse("gate=loud\nnonsense\n"), Settings::default());
    }
}
