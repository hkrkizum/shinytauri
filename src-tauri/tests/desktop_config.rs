use std::path::Path;
use tauri::utils::config::{FrontendDist, WebviewUrl};

fn context() -> tauri::Context<tauri::Wry> {
    tauri::generate_context!()
}

#[test]
fn dev_webview_connects_to_the_local_rhino_server() {
    let ctx = context();
    let url = ctx.config().build.dev_url.as_ref().expect("Rhino dev URL");
    assert_eq!(url.as_str(), "http://127.0.0.1:3838/");
}

#[test]
fn bundled_window_has_a_local_entry_page() {
    let ctx = context();
    let config = ctx.config();
    let Some(FrontendDist::Directory(directory)) = &config.build.frontend_dist else {
        panic!("expected a local frontend directory");
    };
    let window = config.app.windows.first().expect("desktop window");
    let WebviewUrl::App(page) = &window.url else {
        panic!("expected a bundled window URL");
    };
    assert!(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(directory)
            .join(page)
            .is_file()
    );
}
