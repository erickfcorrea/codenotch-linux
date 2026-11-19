//! Local Ollama runtime adapter for Linux.
//!
//! Queries `GET http://127.0.0.1:11434/api/ps` (or a loopback `OLLAMA_HOST`) every 10 s.
//! When Ollama is not running, reports `status: "none"` so the notch stays clean.
//! When running, reports every loaded model with its VRAM/RAM size, quantization, context
//! length, and unload deadline (`expires_at`).

use crate::usage::{LimitWindow, UsageSnapshot};
use crate::AppState;
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

const DEFAULT_BASE: &str = "http://127.0.0.1:11434";
const POLL_SECS: u64 = 10;

static REFRESH: AtomicBool = AtomicBool::new(false);

pub fn request_refresh() {
    REFRESH.store(true, Ordering::Relaxed);
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Deserialize)]
struct PsResponse {
    models: Vec<PsModel>,
}

#[derive(Deserialize)]
struct PsModel {
    name: String,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    size_vram: Option<u64>,
    #[serde(default)]
    context_length: Option<u64>,
    #[serde(default)]
    expires_at: Option<String>,
    #[serde(default)]
    details: Option<PsDetails>,
}

#[derive(Deserialize)]
struct PsDetails {
    #[serde(default)]
    quantization_level: Option<String>,
}

/// Only permits loopback addresses so a malformed environment variable cannot redirect local polling.
pub fn resolve_base_url(raw: Option<&str>) -> String {
    let Some(s) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return DEFAULT_BASE.into();
    };
    let with_scheme = if s.starts_with("http://") || s.starts_with("https://") {
        s.to_string()
    } else {
        format!("http://{s}")
    };
    let trimmed = with_scheme.trim_end_matches('/');
    let after_scheme = trimmed
        .strip_prefix("http://")
        .or_else(|| trimmed.strip_prefix("https://"))
        .unwrap_or("");
    let host_port = after_scheme.split('/').next().unwrap_or("");
    let host = if host_port.starts_with('[') {
        host_port.split(']').next().unwrap_or("").trim_start_matches('[')
    } else {
        host_port.split(':').next().unwrap_or("")
    };
    match host.to_ascii_lowercase().as_str() {
        "127.0.0.1" | "localhost" | "::1" => {
            if host.eq_ignore_ascii_case("localhost") {
                trimmed.replacen("localhost", "127.0.0.1", 1)
            } else {
                trimmed.to_string()
            }
        }
        _ => DEFAULT_BASE.into(),
    }
}

fn format_gib(bytes: u64) -> String {
    let gib = bytes as f64 / (1024.0 * 1024.0 * 1024.0);
    if gib >= 10.0 {
        format!("{gib:.0} GB")
    } else {
        format!("{gib:.1} GB")
    }
}

fn parse_iso_ms(s: &str) -> Option<u64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .and_then(|dt| u64::try_from(dt.timestamp_millis()).ok())
}

pub fn parse_ps_json(body: &str, now: u64) -> Result<UsageSnapshot, String> {
    let parsed: PsResponse = serde_json::from_str(body).map_err(|e| e.to_string())?;
    if parsed.models.is_empty() {
        return Ok(UsageSnapshot {
            accounts: Vec::new(),
            status: "ok".into(),
            windows: vec![LimitWindow {
                id: "loaded".into(),
                label: "Loaded models".into(),
                used: 0.0,
                resets_at: None,
                duration_ms: None,
                count: Some(0),
                derived: false,
                group: None,
                amount: None,
            }],
            fetched_at: now,
            note: "Ollama running · idle".into(),
            backoff_until: 0,
        });
    }

    let mut windows = Vec::with_capacity(parsed.models.len());
    for m in parsed.models {
        let name = m.name.trim();
        if name.is_empty() {
            continue;
        }
        let mut parts = Vec::new();
        if let Some(q) = m
            .details
            .as_ref()
            .and_then(|d| d.quantization_level.as_deref())
            .map(str::trim)
            .filter(|q| !q.is_empty())
        {
            parts.push(q.to_string());
        }
        if let Some(ctx) = m.context_length.filter(|c| *c > 0) {
            if ctx >= 1024 {
                parts.push(format!("{}k ctx", ctx / 1024));
            } else {
                parts.push(format!("{ctx} ctx"));
            }
        }
        let label = if parts.is_empty() {
            name.to_string()
        } else {
            format!("{name} ({})", parts.join(" · "))
        };
        let (used, amount) = match (m.size, m.size_vram) {
            (Some(sz), Some(vram)) if sz > 0 => {
                let frac = (vram as f64 / sz as f64).clamp(0.0, 1.0);
                (frac, Some(format!("{} VRAM / {}", format_gib(vram), format_gib(sz))))
            }
            (Some(sz), _) if sz > 0 => (0.0, Some(format_gib(sz))),
            _ => (0.0, None),
        };
        windows.push(LimitWindow {
            id: format!("ollama:{name}"),
            label,
            used,
            resets_at: m.expires_at.as_deref().and_then(parse_iso_ms),
            duration_ms: None,
            count: None,
            derived: false,
            group: Some("Ollama Local".into()),
            amount,
        });
    }

    let count = windows.len();
    Ok(UsageSnapshot {
        accounts: Vec::new(),
        status: "ok".into(),
        windows,
        fetched_at: now,
        note: format!("{count} local model{} loaded", if count == 1 { "" } else { "s" }),
        backoff_until: 0,
    })
}

fn fetch_once() -> UsageSnapshot {
    let base = resolve_base_url(std::env::var("OLLAMA_HOST").ok().as_deref());
    let url = format!("{base}/api/ps");
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(2))
        .build();
    match agent.get(&url).call() {
        Ok(resp) => match resp.into_string() {
            Ok(body) => parse_ps_json(&body, now_ms()).unwrap_or_else(|e| UsageSnapshot {
                status: "error".into(),
                note: format!("Ollama parse error: {e}"),
                fetched_at: now_ms(),
                ..Default::default()
            }),
            Err(_) => UsageSnapshot {
                status: "absent".into(),
                ..Default::default()
            },
        },
        Err(_) => UsageSnapshot {
            status: "absent".into(),
            ..Default::default()
        },
    }
}

pub fn start(app: AppHandle) {
    std::thread::spawn(move || loop {
        let snap = fetch_once();
        {
            let st = app.state::<AppState>();
            let mut guard = st.ollama.lock().unwrap();
            *guard = snap.clone();
        }
        let _ = app.emit("ollama", &snap);
        for _ in 0..POLL_SECS {
            if REFRESH.swap(false, Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_only_loopback_hosts() {
        assert_eq!(resolve_base_url(None), "http://127.0.0.1:11434");
        assert_eq!(resolve_base_url(Some("localhost:11434")), "http://127.0.0.1:11434");
        assert_eq!(resolve_base_url(Some("http://127.0.0.1:11435/")), "http://127.0.0.1:11435");
        assert_eq!(resolve_base_url(Some("http://evil.example.com:11434")), "http://127.0.0.1:11434");
    }

    #[test]
    fn parses_loaded_ollama_models() {
        let json = r#"{
            "models": [{
                "name": "qwen2.5-coder:14b",
                "size": 9663676416,
                "size_vram": 9663676416,
                "context_length": 32768,
                "expires_at": "2026-11-19T12:30:00Z",
                "details": {"quantization_level": "Q4_K_M"}
            }]
        }"#;
        let snap = parse_ps_json(json, 1_000).expect("valid ps");
        assert_eq!(snap.status, "ok");
        assert_eq!(snap.windows.len(), 1);
        assert_eq!(snap.windows[0].id, "ollama:qwen2.5-coder:14b");
        assert!(snap.windows[0].label.contains("Q4_K_M"));
        assert!(snap.windows[0].label.contains("32k ctx"));
        assert!((snap.windows[0].used - 1.0).abs() < 1e-6);
        assert!(snap.windows[0].resets_at.is_some());
    }

    #[test]
    fn parses_idle_ollama_server() {
        let snap = parse_ps_json(r#"{"models":[]}"#, 1_000).expect("idle");
        assert_eq!(snap.status, "ok");
        assert_eq!(snap.windows[0].count, Some(0));
    }
}
