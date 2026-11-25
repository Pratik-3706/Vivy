// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::fs::OpenOptions;
use std::io::Write;
use std::panic;

fn main() {
    // Global panic hook to capture any unexpected runtime panic into crash log
    panic::set_hook(Box::new(|info| {
        let msg = format!("[Vivy Panic] {}\n", info);
        eprintln!("{}", msg);
        let Some(path) = vivy_core::paths::data_path("vivy_crash.log") else { return };
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(file, "{}", msg);
        }
    }));

    desktop_lib::run();
}
