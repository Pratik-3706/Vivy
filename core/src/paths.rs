//! Where Vivy keeps its files on each system.
//!
//! - Windows: `%LOCALAPPDATA%\Vivy` (data) and `%APPDATA%\Vivy` (CLI config)
//! - Linux: `~/.local/share/vivy` (data) and `~/.config/vivy` (CLI config),
//!   or the `XDG_DATA_HOME` / `XDG_CONFIG_HOME` equivalents.
//!
//! `VIVY_HOME` overrides the data folder (handy for tests or a portable setup).

use std::path::PathBuf;

const APP_DIR: &str = if cfg!(windows) { "Vivy" } else { "vivy" };

/// Settings fallback, chats, pairing identity and logs. Created on demand.
pub fn data_dir() -> Option<PathBuf> {
    let dir = match std::env::var_os("VIVY_HOME") {
        Some(home) if !home.is_empty() => PathBuf::from(home),
        _ => dirs::data_local_dir()?.join(APP_DIR),
    };
    let _ = std::fs::create_dir_all(&dir);
    Some(dir)
}

/// A file or folder inside the data folder.
pub fn data_path(name: &str) -> Option<PathBuf> {
    data_dir().map(|d| d.join(name))
}

/// User-editable configuration (the CLI's look and feel). Created on demand.
pub fn config_dir() -> Option<PathBuf> {
    let dir = match std::env::var_os("VIVY_HOME") {
        Some(home) if !home.is_empty() => PathBuf::from(home),
        _ => dirs::config_dir()?.join(APP_DIR),
    };
    let _ = std::fs::create_dir_all(&dir);
    Some(dir)
}

/// Write a file only the current user can read (keys, tokens). On Windows the
/// per-user profile folder already restricts access.
pub fn write_private(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        f.write_all(contents)?;
        // An existing file keeps its old mode with OpenOptions; tighten it.
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, contents)
    }
}
