// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(all(windows, not(dev)))]
mod offline;

fn main() {
    let builder = tauri::Builder::default();
    #[cfg(all(windows, not(dev)))]
    let builder = builder.setup(offline::start);
    builder
        .run(tauri::generate_context!())
        .expect("error while running shinytauri");
}
