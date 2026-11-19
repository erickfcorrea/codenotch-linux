//! Local LM Studio runtime adapter for Linux.
//!
//! Queries `GET http://127.0.0.1:1234/api/v1/models` (or loopback `LMSTUDIO_HOST`).
//! When LM Studio's server is not running, reports `status: "none"` so the cell stays hidden.
//! Reports every loaded LLM instance (skipping embeddings) with quantization, context length,
//! and on-disk model size.

use crate::usage::{LimitWindow, UsageSnapshot};
use crate::AppState;
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

const DEFAULT_BASE: &str = "http://127.0.0.1:1234";
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
struct ModelsResponse {
    models: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    size_bytes: Option<u64>,
    #[serde(default)]
    quantization: Option<Quant>,
    #[serde(default)]
    max_context_length: Option<u64>,
    #[serde(default)]
    loaded_instances: Option<Vec<LoadedInstance>>,
}

#[derive(Deserialize)]
struct Quant {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct LoadedInstance {
    id: String,
    #[serde(default)]
    config: Option<InstanceConfig>,
}

#[derive(Deserialize)]
struct InstanceConfig {
    #[serde(default)]
    context_length: Option<u64>,
}

fn resolve_base_url(raw: Option<&str>) -> String {
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

pub fn parse_models_json(body: &str, now: u64) -> Result<UsageSnapshot, String> {
    let parsed: ModelsResponse = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let mut windows = Vec::new();
    for m in parsed.models {
        if m.kind.as_deref() != Some("llm") {
            continue;
        }
        let quant = m
            .quantization
            .as_ref()
            .and_then(|q| q.name.as_deref())
            .map(str::trim)
            .filter(|q| !q.is_empty());
        for inst in m.loaded_instances.unwrap_or_default() {
            let id = inst.id.trim();
            if id.is_empty() {
                continue;
            }
            let ctx = inst
                .config
                .as_ref()
                .and_then(|c| c.context_length)
                .or(m.max_context_length);
            let mut parts = Vec::new();
            if let Some(q) = quant {
                parts.push(q.to_string());
            }
            if let Some(c) = ctx.filter(|c| *c > 0) {
                if c >= 1024 {
                    parts.push(format!("{}k ctx", c / 1024));
                } else {
                    parts.push(format!("{c} ctx"));
                }
            }
            let label = if parts.is_empty() {
                id.to_string()
            } else {
                format!("{id} ({})", parts.join(" · "))
            };
            windows.push(LimitWindow {
                id: format!("lmstudio:{id}"),
                label,
                used: 0.0,
                resets_at: None,
                duration_ms: None,
                count: None,
                derived: false,
                group: Some("LM Studio".into()),
                amount: m.size_bytes.filter(|b| *b > 0).map(format_gib),
            });
        }
    }

    if windows.is_empty() {
        return Ok(UsageSnapshot {
            accounts: Vec::new(),
            status: "ok".into(),
            windows: vec![LimitWindow {
                id: "loaded".into(),
                label: "Loaded instances".into(),
                used: 0.0,
                resets_at: None,
                duration_ms: None,
                count: Some(0),
                derived: false,
                group: None,
                amount: None,
            }],
            fetched_at: now,
            note: "LM Studio running · no LLM loaded".into(),
            backoff_until: 0,
        });
    }

    let count = windows.len();
    Ok(UsageSnapshot {
        accounts: Vec::new(),
        status: "ok".into(),
        windows,
        fetched_at: now,
        note: format!("{count} instance{} loaded", if count == 1 { "" } else { "s" }),
        backoff_until: 0,
    })
}

fn fetch_once() -> UsageSnapshot {
    let base = resolve_base_url(std::env::var("LMSTUDIO_HOST").ok().as_deref());
    let url = format!("{base}/api/v1/models");
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(2))
        .build();
    match agent.get(&url).call() {
        Ok(resp) => match resp.into_string() {
            Ok(body) => parse_models_json(&body, now_ms()).unwrap_or_else(|_| UsageSnapshot {
                status: "absent".into(),
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
            let mut guard = st.lmstudio.lock().unwrap();
            *guard = snap.clone();
        }
        let _ = app.emit("lmstudio", &snap);
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
    fn parses_loaded_llm_and_skips_embeddings() {
        let json = r#"{
            "models": [
                {
                    "type": "llm",
                    "key": "qwen3-14b",
                    "size_bytes": 10737418240,
                    "quantization": {"name": "Q4_K_M"},
                    "max_context_length": 65536,
                    "loaded_instances": [{"id": "qwen3-14b", "config": {"context_length": 32768}}]
                },
                {
                    "type": "embedding",
                    "key": "nomic-embed",
                    "loaded_instances": [{"id": "nomic-embed"}]
                }
            ]
        }"#;
        let snap = parse_models_json(json, 1_000).expect("valid models");
        assert_eq!(snap.status, "ok");
        assert_eq!(snap.windows.len(), 1);
        assert_eq!(snap.windows[0].id, "lmstudio:qwen3-14b");
        assert_eq!(snap.windows[0].amount.as_deref(), Some("10 GB"));
        assert!(snap.windows[0].label.contains("32k ctx"));
    }
}
