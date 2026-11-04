//! User preferences, stored as RON in
//! `$XDG_CONFIG_HOME/system-profiler/config.ron`.
//!
//! Missing or unreadable files fall back to defaults, and fields missing
//! from an older file take their default, so the app always starts.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const DEFAULT_ACCENT: &str = "#ffa51a";

// Independent on/off preferences; bools read best in the RON file.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Accent colour as `#rrggbb`.
    pub accent: String,
    /// 0 Graphite, 1 Pure Black, 2 Midnight.
    pub background: u8,
    /// Strength of glows and highlights, 0-100.
    pub glow: u8,
    pub rounded: bool,
    pub compact: bool,
    /// UI zoom in percent: 90, 100 or 110.
    pub text_size: u8,
    pub icons_only_sidebar: bool,
    pub show_hero: bool,
    /// Keep the keyboard backlight in the accent colour.
    pub match_keyboard: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            accent: DEFAULT_ACCENT.to_owned(),
            background: 0,
            glow: 70,
            rounded: false,
            compact: false,
            text_size: 100,
            icons_only_sidebar: false,
            show_hero: true,
            match_keyboard: false,
        }
    }
}

impl Config {
    /// Clamp values a hand-edited file might get wrong.
    #[must_use]
    pub fn sanitized(mut self) -> Self {
        if parse_hex(&self.accent).is_none() {
            DEFAULT_ACCENT.clone_into(&mut self.accent);
        }
        self.background = self.background.min(2);
        self.glow = self.glow.min(100);
        if ![
            90, 100, 110,
        ]
        .contains(&self.text_size)
        {
            self.text_size = 100;
        }
        self
    }
}

/// `#rrggbb` -> (r, g, b).
pub fn parse_hex(hex: &str) -> Option<(u8, u8, u8)> {
    let digits = hex.strip_prefix('#')?;
    if digits.len() != 6 || !digits.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
    Some((byte(0)?, byte(2)?, byte(4)?))
}

pub fn to_hex((r, g, b): (u8, u8, u8)) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Default location of the config file.
pub fn default_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("system-profiler")
        .join("config.ron")
}

pub fn load(path: &Path) -> Config {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Config::default(),
        Err(e) => {
            log::warn!("Cannot read {}: {e}; using defaults", path.display());
            return Config::default();
        }
    };
    ron::from_str::<Config>(&text).map_or_else(
        |e| {
            log::warn!("Ignoring malformed {}: {e}", path.display());
            Config::default()
        },
        Config::sanitized,
    )
}

/// Write atomically: a crash mid-write never leaves a truncated file.
pub fn save(path: &Path, config: &Config) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let text = ron::ser::to_string_pretty(config, ron::ser::PrettyConfig::default())
        .map_err(io::Error::other)?;
    let tmp = path.with_extension("ron.tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("profiler-config-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = temp_dir("missing");
        assert_eq!(load(&dir.join("config.ron")), Config::default());
    }

    #[test]
    fn round_trip() {
        let dir = temp_dir("round-trip");
        let path = dir.join("nested").join("config.ron");
        let config = Config {
            accent: "#22d3ee".to_owned(),
            background: 2,
            glow: 40,
            rounded: true,
            compact: true,
            text_size: 110,
            icons_only_sidebar: true,
            show_hero: false,
            match_keyboard: true,
        };
        save(&path, &config).unwrap();
        assert_eq!(load(&path), config);
        assert!(!path.with_extension("ron.tmp").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn partial_and_bad_files() {
        let dir = temp_dir("partial");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.ron");
        fs::write(&path, "(glow: 250, accent: \"orange\", text_size: 120)").unwrap();
        let config = load(&path);
        assert_eq!(config.glow, 100);
        assert_eq!(config.accent, DEFAULT_ACCENT);
        assert_eq!(config.text_size, 100);
        assert!(config.show_hero, "missing fields take defaults");

        fs::write(&path, "not ron at all").unwrap();
        assert_eq!(load(&path), Config::default());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn hex_colours() {
        assert_eq!(parse_hex("#ffa51a"), Some((255, 165, 26)));
        assert_eq!(parse_hex("ffa51a"), None);
        assert_eq!(parse_hex("#ffa5"), None);
        assert_eq!(parse_hex("#ffa51é"), None);
        assert_eq!(to_hex((255, 165, 26)), "#ffa51a");
    }
}
