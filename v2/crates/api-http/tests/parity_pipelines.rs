//! Parity tests — `api-http` pipelines (F4-05c) vs `plugin/ui/server.cjs`.
//!
//! 10 rotas:
//! - `GET  /api/pipelines`
//! - `POST /api/pipelines/install`
//! - `POST /api/pipelines/create`
//! - `GET  /api/pipelines/state`
//! - `POST /api/pipelines/start`
//! - `POST /api/pipelines/advance`
//! - `POST /api/pipelines/cancel`
//! - `POST /api/pipelines/clear`
//! - `GET  /api/pipelines/runs`
//! - `GET  /api/pipelines/:id`

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
            .env("PROJECT_MEM_PLUGIN_DIR", plugin_path())
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

/// `path` em respostas de create carrega path absoluto do tempdir.
fn strip_path(v: &mut Value) {
    if let Value::Object(m) = v {
        m.remove("path");
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[test]
fn list_matches_catalog() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl("GET", &rurl(sr.port, "/api/pipelines"), None);
    let n = curl("GET", &rurl(sn.port, "/api/pipelines"), None);
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    assert_eq!(r.body, n.body);
}

#[test]
fn get_unknown_404() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl("GET", &rurl(sr.port, "/api/pipelines/does-not-exist"), None);
    let n = curl("GET", &rurl(sn.port, "/api/pipelines/does-not-exist"), None);
    assert_eq!(r.status, 404);
    assert_eq!(n.status, 404);
    assert_eq!(r.body, n.body);
}

#[test]
fn get_known_template() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    // `review-before-merge` existe em `plugin/pipelines/`.
    let r = curl("GET", &rurl(sr.port, "/api/pipelines/review-before-merge"), None);
    let n = curl("GET", &rurl(sn.port, "/api/pipelines/review-before-merge"), None);
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    assert_eq!(r.body, n.body);
}

#[test]
fn install_missing_id_400() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{}"#;
    let r = curl("POST", &rurl(sr.port, "/api/pipelines/install"), Some(body));
    let n = curl("POST", &rurl(sn.port, "/api/pipelines/install"), Some(body));
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
}

#[test]
fn install_unknown_template_400() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"id":"nope"}"#;
    let r = curl("POST", &rurl(sr.port, "/api/pipelines/install"), Some(body));
    let n = curl("POST", &rurl(sn.port, "/api/pipelines/install"), Some(body));
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
}

#[test]
fn install_existing_template() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"id":"review-before-merge"}"#;
    // Primeira vez → installed:true.
    let r1 = curl("POST", &rurl(sr.port, "/api/pipelines/install"), Some(body));
    let n1 = curl("POST", &rurl(sn.port, "/api/pipelines/install"), Some(body));
    assert_eq!(r1.status, 200);
    assert_eq!(n1.status, 200);
    assert_eq!(r1.body, n1.body);
    // Segunda vez → installed:false, exists.
    let r2 = curl("POST", &rurl(sr.port, "/api/pipelines/install"), Some(body));
    let n2 = curl("POST", &rurl(sn.port, "/api/pipelines/install"), Some(body));
    assert_eq!(r2.status, 200);
    assert_eq!(n2.status, 200);
    assert_eq!(r2.body, n2.body);
}

#[test]
fn create_invalid_payload_400() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    // id inválido (maiúsculo).
    let body = r#"{"scope":"project","id":"BAD","label":"x","steps":[{"id":"a","agent":"x"}]}"#;
    let r = curl("POST", &rurl(sr.port, "/api/pipelines/create"), Some(body));
    let n = curl("POST", &rurl(sn.port, "/api/pipelines/create"), Some(body));
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
}

#[test]
fn create_project_round_trip() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"scope":"project","id":"my-pipe","label":"My Pipe","steps":[{"id":"step-a","agent":"manager"}]}"#;
    let mut r = curl("POST", &rurl(sr.port, "/api/pipelines/create"), Some(body));
    let mut n = curl("POST", &rurl(sn.port, "/api/pipelines/create"), Some(body));
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    strip_path(&mut r.body);
    strip_path(&mut n.body);
    assert_eq!(r.body, n.body);
}

#[test]
fn state_empty() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl("GET", &rurl(sr.port, "/api/pipelines/state"), None);
    let n = curl("GET", &rurl(sn.port, "/api/pipelines/state"), None);
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    assert_eq!(r.body, n.body);
}

#[test]
fn start_missing_id_400() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{}"#;
    let r = curl("POST", &rurl(sr.port, "/api/pipelines/start"), Some(body));
    let n = curl("POST", &rurl(sn.port, "/api/pipelines/start"), Some(body));
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
}

#[test]
fn start_unknown_pipeline_400() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"id":"nope"}"#;
    let r = curl("POST", &rurl(sr.port, "/api/pipelines/start"), Some(body));
    let n = curl("POST", &rurl(sn.port, "/api/pipelines/start"), Some(body));
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
}

#[test]
fn advance_missing_step_id_400() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{}"#;
    let r = curl("POST", &rurl(sr.port, "/api/pipelines/advance"), Some(body));
    let n = curl("POST", &rurl(sn.port, "/api/pipelines/advance"), Some(body));
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
}

#[test]
fn advance_no_run_400() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"step_id":"x"}"#;
    let r = curl("POST", &rurl(sr.port, "/api/pipelines/advance"), Some(body));
    let n = curl("POST", &rurl(sn.port, "/api/pipelines/advance"), Some(body));
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
}

#[test]
fn cancel_without_state_returns_null() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{}"#;
    let r = curl("POST", &rurl(sr.port, "/api/pipelines/cancel"), Some(body));
    let n = curl("POST", &rurl(sn.port, "/api/pipelines/cancel"), Some(body));
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    assert_eq!(r.body, n.body);
}

#[test]
fn clear_ok() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl("POST", &rurl(sr.port, "/api/pipelines/clear"), None);
    let n = curl("POST", &rurl(sn.port, "/api/pipelines/clear"), None);
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    assert_eq!(r.body, n.body);
}

#[test]
fn runs_empty() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl("GET", &rurl(sr.port, "/api/pipelines/runs"), None);
    let n = curl("GET", &rurl(sn.port, "/api/pipelines/runs"), None);
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    assert_eq!(r.body, n.body);
}
