//! Start at sign-in on Linux via the XDG autostart directory (~/.config/autostart/codenotch.desktop).
//! Per-user, zero root rights needed, forces GDK_BACKEND=x11 and launches with --silent.

use std::path::PathBuf;

fn desktop_entry_path() -> Option<PathBuf> {
    let dir = dirs::config_dir()?.join("autostart");
    Some(dir.join("codenotch.desktop"))
}

pub fn is_enabled() -> bool {
    desktop_entry_path().is_some_and(|p| p.is_file())
}

pub fn enable() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let path = desktop_entry_path().ok_or("no XDG config directory")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Codenotch\nComment=AI coding assistant usage notch for Linux\nExec=env -u LD_LIBRARY_PATH -u GTK_PATH -u GIO_MODULE_DIR -u GSETTINGS_SCHEMA_DIR GDK_BACKEND=x11 \"{}\" --silent\nIcon=codenotch\nTerminal=false\nCategories=Utility;Development;\nX-GNOME-Autostart-enabled=true\n",
        exe.display()
    );
    std::fs::write(&path, entry).map_err(|e| e.to_string())?;
    Ok("start at sign-in enabled (silent until a session appears)".into())
}

pub fn disable() -> Result<String, String> {
    let path = desktop_entry_path().ok_or("no XDG config directory")?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok("start at sign-in disabled".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok("start at sign-in was not enabled".into())
        }
        Err(e) => Err(e.to_string()),
    }
}
