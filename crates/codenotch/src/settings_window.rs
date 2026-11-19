//! The Linux settings window. Created lazily on open and destroyed on close.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const LABEL: &str = "settings";

pub fn open(app: &AppHandle) {
    let handle = app.clone();
    std::thread::spawn(move || {
        let app = handle.clone();
        let _ = handle.run_on_main_thread(move || open_now(&app));
    });
}

fn open_now(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let theme = crate::theme_choice(app);
    let builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("settings.html".into()))
        .title("Codenotch Settings")
        .inner_size(680.0, 560.0)
        .resizable(true)
        .maximizable(false)
        .decorations(true)
        .center()
        .visible(false)
        .theme(theme)
        .initialization_script(crate::theme_script(crate::resolved_theme(app)));

    match builder.build() {
        Ok(w) => {
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(2));
                if !w.is_visible().unwrap_or(true) {
                    reveal(&w);
                }
            });
        }
        Err(e) => crate::applog(&format!("settings window: {e}")),
    }
}

#[tauri::command]
pub fn settings_ready(app: AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        reveal(&w);
    }
}

fn reveal(w: &tauri::WebviewWindow) {
    let _ = w.show();
    let _ = w.set_focus();
}

#[derive(serde::Serialize)]
pub struct SystemLook {
    mica: bool,
    accent: Vec<String>,
}

#[tauri::command]
pub fn get_system_look() -> SystemLook {
    SystemLook {
        mica: false,
        accent: Vec::new(),
    }
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub fn open_author_page() {
    let _ = std::process::Command::new("xdg-open")
        .arg("https://github.com/erickfcorrea/codenotch-linux")
        .spawn();
}

pub fn follow_theme(_app: &AppHandle, _theme: Option<tauri::Theme>) {}
