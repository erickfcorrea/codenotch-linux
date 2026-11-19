//! Check published Linux releases on GitHub (`erickfcorrea/codenotch-linux`),
//! opening the release page via `xdg-open` when a newer Linux package is available.

use semver::Version;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

const RELEASE_API: &str = "https://api.github.com/repos/erickfcorrea/codenotch-linux/releases/latest";
const RELEASES_PAGE: &str = "https://github.com/erickfcorrea/codenotch-linux/releases/latest";
const UNSET_PUBKEY: &str = "REPLACE_WITH_TAURI_PUBLIC_KEY";
const CHECK_ERROR: &str = "Could not check for updates";
const INSTALL_ERROR: &str = "Could not install the update";

#[derive(Clone, Serialize, Default)]
pub struct UpdateState {
    pub available: Option<String>,
    pub checking: bool,
    pub installing: bool,
    pub checked: bool,
    pub can_install: bool,
    pub message: Option<String>,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
}

static STATE: Mutex<Option<UpdateState>> = Mutex::new(None);

fn state() -> std::sync::MutexGuard<'static, Option<UpdateState>> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

fn set(app: &AppHandle, next: UpdateState) {
    *state() = Some(next.clone());
    let _ = app.emit("update_state", &next);
}

#[tauri::command]
pub fn get_update_state() -> UpdateState {
    state().clone().unwrap_or_default()
}

fn configured(app: &AppHandle) -> bool {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|u| u.get("pubkey"))
        .and_then(|k| k.as_str())
        .is_some_and(|k| !k.is_empty() && k != UNSET_PUBKEY)
}

fn is_linux_asset(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".appimage") || lower.ends_with(".deb") || lower.ends_with("linux-x86_64.tar.gz")
}

fn newer_linux_release(body: &str, current: &Version) -> Result<Option<Version>, String> {
    let release: GithubRelease = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let tag = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name);
    let version = Version::parse(tag).map_err(|e| e.to_string())?;
    if !release.assets.iter().any(|a| is_linux_asset(&a.name)) {
        return Err(format!("release {} has no Linux asset", release.tag_name));
    }
    Ok((version > *current).then_some(version))
}

fn latest_linux_release(current: &Version) -> Result<Option<Version>, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(12))
        .build();
    let resp = match agent
        .get(RELEASE_API)
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", "Codenotch-Linux")
        .call()
    {
        Ok(r) => r,
        Err(ureq::Error::Status(404, _)) => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let body = resp.into_string().map_err(|e| e.to_string())?;
    newer_linux_release(&body, current)
}

fn signed_feed_offer(app: &AppHandle) -> Option<String> {
    match tauri::async_runtime::block_on(async { app.updater()?.check().await }) {
        Ok(Some(update)) => Some(update.version),
        Ok(None) => None,
        Err(e) => {
            crate::applog(&format!("updater: signed feed unavailable ({e})"));
            None
        }
    }
}

pub fn check_on_launch(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(10));
        run_check(&app, false);
    });
}

#[tauri::command]
pub fn check_for_update(app: AppHandle) {
    std::thread::spawn(move || run_check(&app, true));
}

fn run_check(app: &AppHandle, explicit: bool) {
    {
        let mut s = state();
        if s.as_ref().is_some_and(|st| st.checking || st.installing) {
            return;
        }
        let mut next = s.clone().unwrap_or_default();
        next.checking = true;
        next.message = None;
        *s = Some(next.clone());
        let _ = app.emit("update_state", &next);
    }

    let current = Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or_else(|_| Version::new(0, 0, 0));
    let mut next = UpdateState {
        checked: true,
        ..Default::default()
    };

    match latest_linux_release(&current) {
        Ok(Some(ver)) => {
            let signed = if configured(app) { signed_feed_offer(app) } else { None };
            let can_install = signed.as_deref() == Some(ver.to_string().as_str());
            next.available = Some(ver.to_string());
            next.can_install = can_install;
        }
        Ok(None) => {
            next.available = None;
        }
        Err(e) => {
            crate::applog(&format!("updater: check failed ({e})"));
            if explicit {
                next.message = Some(CHECK_ERROR.into());
            }
        }
    }
    set(app, next);
}

#[tauri::command]
pub fn open_update_installer() {
    let _ = std::process::Command::new("xdg-open")
        .arg(RELEASES_PAGE)
        .spawn();
}

#[tauri::command]
pub fn install_update(app: AppHandle) -> Result<(), String> {
    if !configured(&app) {
        open_update_installer();
        return Ok(());
    }
    let handle = app.clone();
    std::thread::spawn(move || {
        let mut cur = get_update_state();
        cur.installing = true;
        cur.message = None;
        set(&handle, cur.clone());

        let outcome: Result<(), String> = tauri::async_runtime::block_on(async {
            let updater = handle.updater().map_err(|e| e.to_string())?;
            let Some(update) = updater.check().await.map_err(|e| e.to_string())? else {
                return Err("no signed update found".into());
            };
            update
                .download_and_install(|_, _| {}, || {})
                .await
                .map_err(|e| e.to_string())
        });

        if let Err(e) = outcome {
            crate::applog(&format!("updater: install failed ({e})"));
            cur.installing = false;
            cur.message = Some(INSTALL_ERROR.into());
            set(&handle, cur);
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_linux_release_assets() {
        let body = r#"{"tag_name":"v1.2.0","assets":[{"name":"codenotch_1.2.0_amd64.deb"}]}"#;
        let cur = Version::new(1, 0, 0);
        assert_eq!(newer_linux_release(body, &cur).unwrap(), Some(Version::new(1, 2, 0)));
    }

    #[test]
    fn ignores_non_linux_only_releases() {
        let body = r#"{"tag_name":"v1.2.0","assets":[{"name":"Codenotch.dmg"}]}"#;
        let cur = Version::new(1, 0, 0);
        assert!(newer_linux_release(body, &cur).is_err());
    }
}
