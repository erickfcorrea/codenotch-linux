//! Linux X11/GTK always-on-top watchdog for the floating notch window.

use std::time::Duration;
use tauri::{AppHandle, Manager, WebviewWindow};

#[allow(dead_code)]
pub fn is_out_of_topmost_band(_window: &WebviewWindow) -> bool {
    false
}

pub fn reassert(window: &WebviewWindow) {
    let _ = window.set_always_on_top(true);
}

pub fn start_watchdog(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(15));
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(w) = handle.get_webview_window("notch") {
                if w.is_visible().unwrap_or(false) {
                    reassert(&w);
                }
            }
        });
    });
}
