//! Parity tests — `api-http` architect/skills/ides (F4-05c) vs `server.cjs`.
//!
//! 5 rotas:
//! - `POST /api/architect/ensure`
//! - `POST /api/architect/generate`
//! - `GET  /api/skills?category=&pack=`
//! - `GET  /api/ides`
//! - `POST /api/ides/apply`

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

// ── Tests ───────────────────────────────────────────────────────────────────

#[test]
fn architect_ensure_ok() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl("POST", &rurl(sr.port, "/api/architect/ensure"), None);
    let n = curl("POST", &rurl(sn.port, "/api/architect/ensure"), None);
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    assert_eq!(r.body, n.body);
}

#[test]
fn architect_generate_missing_spec_400() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{}"#;
    let r = curl("POST", &rurl(sr.port, "/api/architect/generate"), Some(body));
    let n = curl("POST", &rurl(sn.port, "/api/architect/generate"), Some(body));
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
}

#[test]
fn architect_generate_bad_kebab_400() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"slug":"BadSlug","category":"cat"}"#;
    let r = curl("POST", &rurl(sr.port, "/api/architect/generate"), Some(body));
    let n = curl("POST", &rurl(sn.port, "/api/architect/generate"), Some(body));
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
}

#[test]
fn skills_missing_params_400() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl("GET", &rurl(sr.port, "/api/skills"), None);
    let n = curl("GET", &rurl(sn.port, "/api/skills"), None);
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
}

#[test]
fn skills_list_real_pack() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    // `tech/team` existe em `plugin/seeds/` e contém skills reais.
    let r = curl("GET", &rurl(sr.port, "/api/skills?category=tech&pack=team"), None);
    let n = curl("GET", &rurl(sn.port, "/api/skills?category=tech&pack=team"), None);
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    assert_eq!(r.body, n.body);
}

#[test]
fn skills_list_unknown_pack() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl("GET", &rurl(sr.port, "/api/skills?category=zzz&pack=nope"), None);
    let n = curl("GET", &rurl(sn.port, "/api/skills?category=zzz&pack=nope"), None);
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    assert_eq!(r.body, n.body);
}

#[test]
fn ides_list() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl("GET", &rurl(sr.port, "/api/ides"), None);
    let n = curl("GET", &rurl(sn.port, "/api/ides"), None);
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    assert_eq!(r.body, n.body);
}

#[test]
fn ides_apply_missing_ide_400() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{}"#;
    let r = curl("POST", &rurl(sr.port, "/api/ides/apply"), Some(body));
    let n = curl("POST", &rurl(sn.port, "/api/ides/apply"), Some(body));
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
}

#[test]
fn ides_apply_unsupported_ide_400() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"ide":"unknown-ide"}"#;
    let r = curl("POST", &rurl(sr.port, "/api/ides/apply"), Some(body));
    let n = curl("POST", &rurl(sn.port, "/api/ides/apply"), Some(body));
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
}
