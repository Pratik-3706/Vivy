//! Diagnostic messages. They go to `vivy.log` in the data folder rather than
//! the terminal, so they never interrupt the CLI's output. Set `VIVY_DEBUG=1`
//! to also print them to stderr.

use std::io::Write;
use std::sync::Mutex;

/// Larger logs are started over, so the file never grows without bound.
const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;

static LOCK: Mutex<()> = Mutex::new(());

pub fn write(line: &str) {
    if std::env::var_os("VIVY_DEBUG").is_some_and(|v| v != "0") {
        eprintln!("{line}");
    }
    let Some(path) = crate::paths::data_path("vivy.log") else { return };
    let _guard = LOCK.lock();
    let truncate = std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_LOG_BYTES);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(!truncate)
        .write(true)
        .truncate(truncate)
        .open(&path);
    if let Ok(mut f) = file {
        let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(f, "{stamp} {line}");
    }
}

/// `vlog!("...", args)`: format and write one log line.
#[macro_export]
macro_rules! vlog {
    ($($arg:tt)*) => {
        $crate::log::write(&format!($($arg)*))
    };
}
