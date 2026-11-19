//! codenotch-hook (Linux): minimal zero-dependency client invoked by Claude Code hooks.
//!
//! Duties:
//!   1) Forward the hook event name, parent PID, local X-Codenotch-Token, and stdin JSON
//!      to the local Codenotch HTTP server on 127.0.0.1:<port>.
//!   2) Auto-launch the main `codenotch` binary detached (with GDK_BACKEND=x11 and snap env
//!      sanitized) if it is not yet running, then retry briefly.
//!
//! Iron rule: never block Claude Code — ~2 s total budget, every failure exits 0 silently.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

const DEFAULT_PORT: u16 = 48666;
const MAX_STDIN: u64 = 256 * 1024;

fn main() {
    let event = std::env::args().nth(1).unwrap_or_else(|| "ping".into());

    let mut body = String::new();
    let _ = std::io::stdin().take(MAX_STDIN).read_to_string(&mut body);

    let cfg_dir = config_dir();
    let port = read_port(cfg_dir.as_deref());
    let mut token = read_token(cfg_dir.as_deref());
    let ppid = std::os::unix::process::parent_id();

    if send(port, &token, &event, ppid, &body).is_ok() {
        return;
    }

    spawn_main();
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(100));
        if token.is_empty() {
            token = read_token(cfg_dir.as_deref());
        }
        if send(port, &token, &event, ppid, &body).is_ok() {
            return;
        }
    }
}

/// Linux XDG config directory: $XDG_CONFIG_HOME/codenotch or ~/.config/codenotch
fn config_dir() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg).join("codenotch"));
        }
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config").join("codenotch"))
}

fn read_token(cfg_dir: Option<&Path>) -> String {
    let Some(dir) = cfg_dir else {
        return String::new();
    };
    std::fs::read_to_string(dir.join("auth_token"))
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn read_port(cfg_dir: Option<&Path>) -> u16 {
    let Some(dir) = cfg_dir else {
        return DEFAULT_PORT;
    };
    let Ok(txt) = std::fs::read_to_string(dir.join("config.json")) else {
        return DEFAULT_PORT;
    };
    parse_port_from_json(&txt).unwrap_or(DEFAULT_PORT)
}

fn parse_port_from_json(txt: &str) -> Option<u16> {
    let i = txt.find("\"port\"")?;
    let digits: String = txt[i + 6..]
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

fn build_request(event: &str, ppid: u32, token: &str, body: &str) -> String {
    let auth_header = if !token.is_empty() {
        format!("X-Codenotch-Token: {token}\r\n")
    } else {
        String::new()
    };
    format!(
        "POST /event?e={event}&ppid={ppid} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{auth_header}Connection: close\r\n\r\n{body}",
        body.len(),
    )
}

fn send(port: u16, token: &str, event: &str, ppid: u32, body: &str) -> std::io::Result<()> {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(300))?;
    s.set_write_timeout(Some(Duration::from_millis(700)))?;
    s.set_read_timeout(Some(Duration::from_millis(700)))?;
    let req = build_request(event, ppid, token, body);
    s.write_all(req.as_bytes())?;
    let mut buf = [0u8; 64];
    let _ = s.read(&mut buf);
    Ok(())
}

/// Launches the sibling `codenotch` binary detached with X11/XWayland env hygiene.
fn spawn_main() {
    let Ok(me) = std::env::current_exe() else { return };
    let Some(dir) = me.parent() else { return };
    let exe = dir.join("codenotch");
    if !exe.exists() {
        return;
    }
    let mut cmd = std::process::Command::new(exe);
    cmd.env("GDK_BACKEND", "x11")
        .env_remove("LD_LIBRARY_PATH")
        .env_remove("GTK_PATH")
        .env_remove("GIO_MODULE_DIR")
        .env_remove("GSETTINGS_SCHEMA_DIR")
        .env_remove("LOCPATH")
        .env_remove("GDK_PIXBUF_MODULE_FILE")
        .env_remove("GDK_PIXBUF_MODULEDIR")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let _ = cmd.spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_port_from_config_json() {
        assert_eq!(parse_port_from_json(r#"{"port": 49123, "scale": 1.0}"#), Some(49123));
        assert_eq!(parse_port_from_json(r#"{"scale": 1.0}"#), None);
    }

    #[test]
    fn builds_authenticated_http_request() {
        let req = build_request("running", 1234, "deadbeef", r#"{"session_id":"s1"}"#);
        assert!(req.starts_with("POST /event?e=running&ppid=1234 HTTP/1.1\r\n"));
        assert!(req.contains("X-Codenotch-Token: deadbeef\r\n"));
        assert!(req.ends_with(r#"{"session_id":"s1"}"#));
    }

    #[test]
    fn omits_auth_header_when_token_empty() {
        let req = build_request("done", 42, "", "{}");
        assert!(!req.contains("X-Codenotch-Token:"));
    }

    #[test]
    fn reads_linux_xdg_token_and_port() {
        let tmp = std::env::temp_dir().join(format!("cn-hook-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        std::fs::write(tmp.join("auth_token"), "  tok123 \n").unwrap();
        std::fs::write(tmp.join("config.json"), r#"{"port": 48777}"#).unwrap();
        assert_eq!(read_token(Some(&tmp)), "tok123");
        assert_eq!(read_port(Some(&tmp)), 48777);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
