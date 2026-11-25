//! The CLI's own settings: how Matsumoto looks and how the terminal is used.
//! Stored as `cli.toml` in the config folder (`vivy status` shows where).
//! API keys and the model are shared with the desktop app and live elsewhere.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::matsumoto::{self, Palette};
use crate::term::Rgb;

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct CliConfig {
    pub matsumoto: MatsumotoConfig,
    pub terminal: TerminalConfig,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct MatsumotoConfig {
    /// classic, night, sakura, mint, sunset or retro.
    pub theme: String,
    /// tiny, small, medium or large.
    pub size: String,
    /// "pixel" (colour half-blocks) or "ascii".
    pub style: String,
    /// Float, blink and turn; off shows a still picture.
    pub animate: bool,
    /// Overrides for the theme's colours, as #rrggbb.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eye: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lens: Option<String>,
}

impl Default for MatsumotoConfig {
    fn default() -> Self {
        Self {
            theme: "classic".into(),
            size: "medium".into(),
            style: "pixel".into(),
            animate: true,
            body: None,
            eye: None,
            lens: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct TerminalConfig {
    /// auto, truecolor, 256 or none.
    pub color: String,
    /// Show Matsumoto and the summary when a chat starts.
    pub banner: bool,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self { color: "auto".into(), banner: true }
    }
}

const HEADER: &str = "\
# Vivy terminal settings. Edit by hand or with `vivy matsumoto --help`.
#
# [matsumoto]
#   theme   = classic | night | sakura | mint | sunset | retro
#   size    = tiny | small | medium | large
#   style   = pixel | ascii
#   animate = true | false
#   body / eye / lens = \"#rrggbb\" to override the theme's colours
#
# [terminal]
#   color  = auto | truecolor | 256 | none
#   banner = true | false

";

pub fn path() -> Option<PathBuf> {
    vivy_core::paths::config_dir().map(|d| d.join("cli.toml"))
}

impl CliConfig {
    /// Missing or unreadable files give the defaults (a broken file is reported).
    pub fn load() -> Self {
        let Some(path) = path() else { return Self::default() };
        match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
                eprintln!("Ignoring {} (using defaults): {e}", path.display());
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<PathBuf, String> {
        let path = path().ok_or("No config folder on this system.")?;
        let body = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&path, format!("{HEADER}{body}")).map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
        Ok(path)
    }
}

/// Everything needed to draw Matsumoto, resolved from the config.
#[derive(Clone, Copy)]
pub struct Look {
    pub palette: Palette,
    pub edge: u32,
    pub animate: bool,
    pub pixel: bool,
}

impl MatsumotoConfig {
    pub fn look(&self) -> Look {
        let theme = matsumoto::theme(&self.theme).unwrap_or(&matsumoto::THEMES[0]);
        let pick = |over: &Option<String>, base: Rgb| over.as_deref().and_then(Rgb::parse).unwrap_or(base);
        Look {
            palette: Palette::new(pick(&self.body, theme.body), pick(&self.eye, theme.eye), pick(&self.lens, theme.lens)),
            edge: matsumoto::edge_for(&self.size).unwrap_or(16),
            animate: self.animate,
            pixel: !self.style.eq_ignore_ascii_case("ascii") && crate::term::unicode_ok(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_tolerates_missing_fields() {
        let mut c = CliConfig::default();
        c.matsumoto.theme = "sakura".into();
        c.matsumoto.eye = Some("#00ff00".into());
        let text = toml::to_string_pretty(&c).unwrap();
        let back: CliConfig = toml::from_str(&text).unwrap();
        assert_eq!(back.matsumoto.theme, "sakura");
        assert_eq!(back.matsumoto.eye.as_deref(), Some("#00ff00"));

        let partial: CliConfig = toml::from_str("[matsumoto]\nsize = \"large\"\n").unwrap();
        assert_eq!(partial.matsumoto.size, "large");
        assert_eq!(partial.matsumoto.theme, "classic");
        assert!(partial.terminal.banner);
    }

    #[test]
    fn overrides_beat_the_theme() {
        let mut m = MatsumotoConfig::default();
        m.eye = Some("#ff0000".into());
        assert_eq!(m.look().palette.eye, Rgb(255, 0, 0));
        m.eye = Some("not a colour".into());
        assert_eq!(m.look().palette.eye, matsumoto::theme("classic").unwrap().eye);
    }
}
