//! Parity tests — `api-http` destructive rotas (F4-05c) vs `plugin/ui/server.cjs`.
//!
//! 4 rotas:
//! - `GET  /api/destructive/patterns`
//! - `PUT  /api/destructive/patterns`
//! - `POST /api/destructive/test`
//! - `POST /api/destructive/core/toggle`

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use tempfile::TempDir;

const PLUGIN_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../plugin");

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn curl_available() -> bool {
    Command::new("curl")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn plugin_path() -> PathBuf {
    PathBuf::from(PLUGIN_DIR)
}

fn cli_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_pmem-http"))
}

fn free_port() -> u16 {
    let l = TcpListener::bind(("127.0.0.1", 0)).expect("bind :0");
    l.local_addr().unwrap().port()
}

fn make_project(tmp: &Path) -> PathBuf {
    let mem = tmp.join(".claude-project-memory");
    std::fs::create_dir_all(&mem).unwrap();
    mem
}

struct Server {
    child: Child,
    port: u16,
}

impl Server {
    fn spawn_node(cwd: &Path, port: u16) -> Self {
        let server_cjs = plugin_path().join("ui").join("server.cjs");
        let child = Command::new("node")
            .arg(&server_cjs)
            .env("PROJECT_MEM_PORT", port.to_string())
            .env("CLAUDE_CWD", cwd)
            .env("PWD", cwd)
            .current_dir(cwd)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn node server.cjs");
        Self { child, port }
    }

    fn spawn_rust(cwd: &Path, port: u16) -> Self {
        let child = Command::new(cli_bin())
            .env("PROJECT_MEM_PORT", port.to_string())
            .env("CLAUDE_CWD", cwd)
            .env("PWD", cwd)
            .current_dir(cwd)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn pmem-http");
        Self { child, port }
    }

    fn wait_ready(&self) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if std::net::TcpStream::connect(("127.0.0.1", self.port)).is_ok() {
                return true;
            }
            thread::sleep(Duration::from_millis(50));
        }
        false
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Debug)]
struct Response {
    status: u16,
    body: Value,
}

fn curl(method: &str, url: &str, body: Option<&str>) -> Response {
    let mut cmd = Command::new("curl");
    cmd.arg("-s")
        .arg("-o")
        .arg("-")
        .arg("-w")
        .arg("\n%{http_code}")
        .arg("-X")
        .arg(method)
        .arg(url)
        .arg("-H")
        .arg("Content-Type: application/json");
    if let Some(b) = body {
        cmd.arg("-d").arg(b);
    }
    let out = cmd.output().expect("curl");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let (body, code) = stdout
        .rsplit_once('\n')
        .map(|(b, c)| (b.to_string(), c.to_string()))
        .unwrap_or_else(|| (stdout.clone(), "0".into()));
    let status: u16 = code.trim().parse().unwrap_or(0);
    let body: Value = serde_json::from_str(body.trim()).unwrap_or(Value::Null);
    Response { status, body }
}

/// Patterns de usuário carregam `id` (random hex) + `created_at` (timestamp) —
/// não dá pra comparar byte-a-byte. Removemos antes de comparar.
fn strip_volatile_user_pattern(v: &mut Value) {
    if let Value::Object(m) = v {
        m.remove("id");
        m.remove("created_at");
    }
}

/// `saveUserPatterns` devolve `{ ok, patterns: [...] }` — percorre `patterns`.
fn strip_patterns_list(v: &mut Value) {
    if let Some(arr) = v.get_mut("patterns").and_then(Value::as_array_mut) {
        for p in arr {
            strip_volatile_user_pattern(p);
        }
    }
}

fn both_servers() -> Option<(TempDir, TempDir, Server, Server)> {
    if !node_available() || !curl_available() {
        eprintln!("skipping — node or curl unavailable");
        return None;
    }
    let tmp_r = TempDir::new().unwrap();
    let tmp_n = TempDir::new().unwrap();
    let _ = make_project(tmp_r.path());
    let _ = make_project(tmp_n.path());
    let p_r = free_port();
    let p_n = free_port();
    let s_r = Server::spawn_rust(tmp_r.path(), p_r);
    let s_n = Server::spawn_node(tmp_n.path(), p_n);
    if !s_r.wait_ready() || !s_n.wait_ready() {
        eprintln!("skipping — server failed to bind");
        return None;
    }
    Some((tmp_r, tmp_n, s_r, s_n))
}

fn rurl(port: u16, path: &str) -> String {
    format!("http://127.0.0.1:{port}{path}")
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[test]
fn patterns_get_defaults() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl("GET", &rurl(sr.port, "/api/destructive/patterns"), None);
    let n = curl("GET", &rurl(sn.port, "/api/destructive/patterns"), None);
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    // Em projeto novo: user=[], core=CORE_PATTERNS (ids idênticos dos 2 lados).
    assert_eq!(r.body, n.body);
}

#[test]
fn patterns_put_round_trip() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"patterns":[{"label":"my secret","text":"super-*","kind":"literal","severity":"high"}]}"#;
    let mut r = curl(
        "PUT",
        &rurl(sr.port, "/api/destructive/patterns"),
        Some(body),
    );
    let mut n = curl(
        "PUT",
        &rurl(sn.port, "/api/destructive/patterns"),
        Some(body),
    );
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    strip_patterns_list(&mut r.body);
    strip_patterns_list(&mut n.body);
    assert_eq!(r.body, n.body);
}

#[test]
fn patterns_put_rejects_non_array() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"patterns":"not-an-array"}"#;
    let r = curl(
        "PUT",
        &rurl(sr.port, "/api/destructive/patterns"),
        Some(body),
    );
    let n = curl(
        "PUT",
        &rurl(sn.port, "/api/destructive/patterns"),
        Some(body),
    );
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
}

#[test]
fn test_detect_empty_text() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"text":""}"#;
    let r = curl("POST", &rurl(sr.port, "/api/destructive/test"), Some(body));
    let n = curl("POST", &rurl(sn.port, "/api/destructive/test"), Some(body));
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    assert_eq!(r.body, n.body);
}

#[test]
fn test_detect_rm_rf() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    // `rm -rf /` é pattern core. Deve bater em ambos.
    let body = r#"{"text":"rm -rf /"}"#;
    let r = curl("POST", &rurl(sr.port, "/api/destructive/test"), Some(body));
    let n = curl("POST", &rurl(sn.port, "/api/destructive/test"), Some(body));
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    // matched=true nos dois. Hits individuais podem variar em ordem/slice.
    assert_eq!(r.body.get("matched"), Some(&Value::Bool(true)));
    assert_eq!(n.body.get("matched"), Some(&Value::Bool(true)));
}

#[test]
fn core_toggle_unknown_id_400() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"id":"does-not-exist","disabled":true}"#;
    let r = curl(
        "POST",
        &rurl(sr.port, "/api/destructive/core/toggle"),
        Some(body),
    );
    let n = curl(
        "POST",
        &rurl(sn.port, "/api/destructive/core/toggle"),
        Some(body),
    );
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
}

#[test]
fn core_toggle_missing_id_400() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"disabled":true}"#;
    let r = curl(
        "POST",
        &rurl(sr.port, "/api/destructive/core/toggle"),
        Some(body),
    );
    let n = curl(
        "POST",
        &rurl(sn.port, "/api/destructive/core/toggle"),
        Some(body),
    );
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
}
