//! Colours and text styles, with fallbacks for terminals that can't show
//! them. Respects `NO_COLOR` and the `color` setting in the CLI config.

use std::io::IsTerminal;
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// `#rrggbb` or `rrggbb`.
    pub fn parse(s: &str) -> Option<Rgb> {
        let h = s.trim().trim_start_matches('#');
        if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let v = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
        Some(Rgb(v(0)?, v(2)?, v(4)?))
    }

    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }

    pub fn luma(self) -> f32 {
        0.299 * self.0 as f32 + 0.587 * self.1 as f32 + 0.114 * self.2 as f32
    }

    /// `t` of the way from self to `other`.
    pub fn mix(self, other: Rgb, t: f32) -> Rgb {
        let f = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round().clamp(0.0, 255.0) as u8;
        Rgb(f(self.0, other.0), f(self.1, other.1), f(self.2, other.2))
    }

    /// Nearest colour in the xterm 256-colour palette.
    fn ansi256(self) -> u8 {
        let Rgb(r, g, b) = self;
        if r == g && g == b {
            if r < 8 {
                return 16;
            }
            if r > 248 {
                return 231;
            }
            return 232 + ((r as u16 - 8) * 24 / 247) as u8;
        }
        let q = |v: u8| ((v as u16 * 5 + 127) / 255) as u8;
        16 + 36 * q(r) + 6 * q(g) + q(b)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorMode {
    TrueColor,
    Ansi256,
    None,
}

static MODE: OnceLock<ColorMode> = OnceLock::new();

/// Decide once, from the config setting ("auto", "truecolor", "256", "none").
pub fn init(setting: &str) {
    // Escape codes need switching on in the Windows console, even when the
    // colour mode is forced in the config.
    #[cfg(windows)]
    let _ = crossterm::ansi_support::supports_ansi();
    // NO_COLOR (https://no-color.org) beats every setting.
    if std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
        let _ = MODE.set(ColorMode::None);
        return;
    }
    let mode = match setting.trim().to_ascii_lowercase().as_str() {
        "truecolor" | "24bit" => ColorMode::TrueColor,
        "256" | "ansi256" => ColorMode::Ansi256,
        "none" | "off" | "never" => ColorMode::None,
        _ => detect(),
    };
    let _ = MODE.set(mode);
}

pub fn mode() -> ColorMode {
    *MODE.get_or_init(detect)
}

fn detect() -> ColorMode {
    if std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) || !std::io::stdout().is_terminal() {
        return ColorMode::None;
    }
    // Turns on escape-code support in the Windows console; false if unavailable.
    #[cfg(windows)]
    if !crossterm::ansi_support::supports_ansi() {
        return ColorMode::None;
    }
    let env = |k: &str| std::env::var(k).unwrap_or_default().to_ascii_lowercase();
    let colorterm = env("COLORTERM");
    if colorterm.contains("truecolor") || colorterm.contains("24bit") {
        return ColorMode::TrueColor;
    }
    // Windows 10+ consoles, Windows Terminal and the common modern terminals.
    if cfg!(windows)
        || std::env::var_os("WT_SESSION").is_some()
        || matches!(env("TERM_PROGRAM").as_str(), "vscode" | "iterm.app" | "wezterm" | "ghostty" | "warpterminal")
    {
        return ColorMode::TrueColor;
    }
    if env("TERM") == "dumb" {
        return ColorMode::None;
    }
    ColorMode::Ansi256
}

/// Whether Unicode block characters are likely to display.
pub fn unicode_ok() -> bool {
    if cfg!(windows) {
        return true; // Windows 10+ consoles render them with the default fonts.
    }
    let env = |k: &str| std::env::var(k).unwrap_or_default().to_ascii_lowercase();
    let lang = format!("{}{}{}", env("LC_ALL"), env("LC_CTYPE"), env("LANG"));
    lang.is_empty() || lang.contains("utf")
}

pub struct Color;

impl Color {
    pub const RESET: &'static str = "\x1b[0m";

    pub fn fg(c: Rgb, mode: ColorMode) -> String {
        match mode {
            ColorMode::TrueColor => format!("\x1b[38;2;{};{};{}m", c.0, c.1, c.2),
            ColorMode::Ansi256 => format!("\x1b[38;5;{}m", c.ansi256()),
            ColorMode::None => String::new(),
        }
    }

    pub fn bg(c: Rgb, mode: ColorMode) -> String {
        match mode {
            ColorMode::TrueColor => format!("\x1b[48;2;{};{};{}m", c.0, c.1, c.2),
            ColorMode::Ansi256 => format!("\x1b[48;5;{}m", c.ansi256()),
            ColorMode::None => String::new(),
        }
    }
}

// ---- Text styles ---------------------------------------------------------------

fn wrap(code: &str, text: &str) -> String {
    if mode() == ColorMode::None {
        text.to_string()
    } else {
        format!("\x1b[{code}m{text}\x1b[0m")
    }
}

pub fn bold(t: &str) -> String {
    wrap("1", t)
}
pub fn dim(t: &str) -> String {
    wrap("2", t)
}
pub fn italic_dim(t: &str) -> String {
    wrap("2;3", t)
}
pub fn red(t: &str) -> String {
    wrap("31", t)
}
pub fn green(t: &str) -> String {
    wrap("32", t)
}
pub fn yellow(t: &str) -> String {
    wrap("33", t)
}
pub fn cyan(t: &str) -> String {
    wrap("36", t)
}

/// Text in an arbitrary colour (falls back to plain text).
pub fn tint(t: &str, c: Rgb) -> String {
    match mode() {
        ColorMode::None => t.to_string(),
        m => format!("{}{t}{}", Color::fg(c, m), Color::RESET),
    }
}

/// Raw escape codes for the streaming renderer, empty without colour.
pub fn code(code: &str) -> String {
    if mode() == ColorMode::None { String::new() } else { format!("\x1b[{code}m") }
}

/// Visible width of a string, ignoring escape codes. Good enough for the
/// ASCII-heavy status line.
pub fn visible_width(s: &str) -> usize {
    let mut width = 0;
    let mut in_escape = false;
    for ch in s.chars() {
        if in_escape {
            if ch.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else if ch == '\x1b' {
            in_escape = true;
        } else {
            width += 1;
        }
    }
    width
}

/// Whether output goes to a terminal (animations and live lines only then).
pub fn is_tty() -> bool {
    std::io::stdout().is_terminal()
}

pub fn width() -> usize {
    crossterm::terminal::size().map(|(w, _)| w as usize).unwrap_or(80).max(20)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_colours() {
        assert_eq!(Rgb::parse("#3d8bff"), Some(Rgb(0x3d, 0x8b, 0xff)));
        assert_eq!(Rgb::parse("3D8BFF"), Some(Rgb(0x3d, 0x8b, 0xff)));
        assert_eq!(Rgb::parse("#fff"), None);
        assert_eq!(Rgb(1, 2, 3).hex(), "#010203");
    }

    #[test]
    fn maps_to_the_256_palette() {
        assert_eq!(Rgb(255, 0, 0).ansi256(), 196);
        assert_eq!(Rgb(0, 0, 0).ansi256(), 16);
        assert_eq!(Rgb(255, 255, 255).ansi256(), 231);
    }

    #[test]
    fn measures_without_escapes() {
        assert_eq!(visible_width("\x1b[1mhi\x1b[0m there"), 8);
    }
}
