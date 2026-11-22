// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(all(windows, not(dev)))]
mod offline;
#[cfg(all(windows, any(not(dev), test)))]
mod quarto;
#[cfg(any(test, all(windows, not(dev))))]
mod readiness;
#[cfg(any(test, all(windows, not(dev))))]
mod startup;

fn main() {
    let builder = tauri::Builder::default();
    #[cfg(all(windows, not(dev)))]
    let builder = builder
        .setup(offline::start)
        .on_page_load(offline::page_loaded);
    builder
        .run(tauri::generate_context!())
        .expect("error while running shinytauri");
}
