//! The desktop shell. Starts the headless engine process and shows the web UI.
//! The UI reaches the engine only through its API, exactly as in a browser.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![forbid(unsafe_code)]

use std::process::{Child, Command};

fn main() {
    let mut engine = start_engine();
    tauri::Builder::default()
        .build(tauri::generate_context!())
        .expect("the window could not be created")
        .run(move |_, event| {
            if let tauri::RunEvent::Exit = event
                && let Some(engine) = engine.as_mut()
            {
                let _ = engine.kill();
                let _ = engine.wait();
            }
        });
}

/// Cargo builds the engine binary into the same folder as this one.
/// If it does not start, the UI shows "disconnected".
fn start_engine() -> Option<Child> {
    let name = format!("bossylights-engine{}", std::env::consts::EXE_SUFFIX);
    let path = std::env::current_exe().ok()?.with_file_name(name);
    Command::new(&path)
        .spawn()
        .inspect_err(|error| eprintln!("bossylights: cannot start {}: {error}", path.display()))
        .ok()
}
