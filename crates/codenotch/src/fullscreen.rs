//! Linux X11/XWayland fullscreen detector via EWMH `_NET_WM_STATE_FULLSCREEN`.
//! Folds the notch automatically when a fullscreen game/video/presentation takes over
//! and the `fold_for_full_screen` setting is enabled.

use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

use crate::{AppState, Screen};

type Rect = (i32, i32, i32, i32);

static FILLED: Mutex<Option<Rect>> = Mutex::new(None);
static HERE: AtomicBool = AtomicBool::new(false);
static FOLDING: AtomicBool = AtomicBool::new(false);

const POLL: Duration = Duration::from_millis(500);

pub fn filled(s: &Screen) -> bool {
    *FILLED.lock().unwrap() == Some((s.x, s.y, s.w, s.h))
}

pub fn apply(mut s: Screen) -> Screen {
    if filled(&s) {
        s.work = (s.x, s.y, s.w, s.h);
    }
    s
}

pub fn folding() -> bool {
    FOLDING.load(SeqCst)
}

pub fn tell(app: &AppHandle) {
    let on = app.state::<AppState>().cfg.lock().unwrap().fold_for_full_screen;
    let fold = on && HERE.load(SeqCst);
    if FOLDING.swap(fold, SeqCst) != fold {
        let _ = app.emit_to("notch", "full_screen", fold);
    }
}

pub fn start(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(POLL);
        let Some(is_fs) = crate::focus::active_window_is_fullscreen() else {
            continue;
        };
        let next = if is_fs {
            crate::attached_screen(&app).map(|s| (s.x, s.y, s.w, s.h))
        } else {
            None
        };
        let changed = {
            let mut guard = FILLED.lock().unwrap();
            if *guard != next {
                *guard = next;
                true
            } else {
                false
            }
        };
        if changed {
            HERE.store(is_fs, SeqCst);
            tell(&app);
            crate::place_notch(&app);
        }
    });
}
