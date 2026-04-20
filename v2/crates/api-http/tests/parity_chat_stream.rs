//! Parity tests — `GET /api/chat/stream` SSE (F4-05f-4).
//!
//! Cobre:
//! - 400 missing agent
//! - Snapshot inicial (linhas existentes) + evento `ready`
//! - Append após connect → evento `msg` para delta
//! - `since_idx` pula linhas antigas
//!
//! Testa comportamento via curl com `--max-time 2` — lê SSE até timeout,
//! então compara eventos coletados.

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
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

fn both_servers() -> Option<(TempDir, TempDir, PathBuf, PathBuf, Server, Server)> {
    if !node_available() || !curl_available() {
        eprintln!("skipping — node or curl unavailable");
        return None;
    }
    let tmp_r = TempDir::new().unwrap();
    let tmp_n = TempDir::new().unwrap();
    let mem_r = make_project(tmp_r.path());
    let mem_n = make_project(tmp_n.path());
    let p_r = free_port();
    let p_n = free_port();
    let s_r = Server::spawn_rust(tmp_r.path(), p_r);
    let s_n = Server::spawn_node(tmp_n.path(), p_n);
    if !s_r.wait_ready() || !s_n.wait_ready() {
        eprintln!("skipping — server failed to bind");
        return None;
    }
    Some((tmp_r, tmp_n, mem_r, mem_n, s_r, s_n))
}

fn rurl(port: u16, path: &str) -> String {
    format!("http://127.0.0.1:{port}{path}")
}

/// Curl com max-time; devolve stdout + status.
fn curl_sse(url: &str, max_time_secs: u64) -> String {
    let out = Command::new("curl")
        .arg("-s")
        .arg("-N")
        .arg("--max-time")
        .arg(max_time_secs.to_string())
        .arg(url)
        .output()
        .expect("curl");
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// Parse SSE stdout em lista `(event, data_json)`.
fn parse_sse(raw: &str) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    let mut current_event = String::new();
    let mut current_data = String::new();
    for line in raw.split('\n') {
        if line.starts_with("event:") {
            current_event = line.trim_start_matches("event:").trim().to_string();
        } else if line.starts_with("data:") {
            current_data = line.trim_start_matches("data:").trim().to_string();
        } else if line.is_empty() {
            if !current_event.is_empty() {
                let data: Value = serde_json::from_str(&current_data).unwrap_or(Value::Null);
                out.push((current_event.clone(), data));
                current_event.clear();
                current_data.clear();
            }
        }
    }
    out
}

fn seed_chat(mem_dir: &Path, agent: &str, entries: &[Value]) {
    let dir = mem_dir.join("agent_chats");
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = String::new();
    for e in entries {
        s.push_str(&serde_json::to_string(e).unwrap());
        s.push('\n');
    }
    std::fs::write(dir.join(format!("{agent}.jsonl")), s).unwrap();
}

fn curl_status(url: &str) -> u16 {
    let out = Command::new("curl")
        .arg("-s")
        .arg("-o")
        .arg("/dev/null")
        .arg("-w")
        .arg("%{http_code}")
        .arg(url)
        .output()
        .expect("curl");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .unwrap_or(0)
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[test]
fn stream_missing_agent_400() {
    let Some((_tr, _tn, _mr, _mn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl_status(&rurl(sr.port, "/api/chat/stream"));
    let n = curl_status(&rurl(sn.port, "/api/chat/stream"));
    assert_eq!(r, 400);
    assert_eq!(n, 400);
}

#[test]
fn stream_initial_snapshot_and_ready() {
    let Some((_tr, _tn, mr, mn, sr, sn)) = both_servers() else {
        return;
    };
    let entries = vec![
        json!({ "role": "user", "content": "oi" }),
        json!({ "role": "agent", "content": "tudo bem?" }),
    ];
    seed_chat(&mr, "x", &entries);
    seed_chat(&mn, "x", &entries);
    let r_raw = curl_sse(&rurl(sr.port, "/api/chat/stream?agent=x"), 1);
    let n_raw = curl_sse(&rurl(sn.port, "/api/chat/stream?agent=x"), 1);
    let r_events = parse_sse(&r_raw);
    let n_events = parse_sse(&n_raw);

    // Filter out ping heartbeats (inexistente nesses primeiros 1s)
    let r_non_ping: Vec<&(String, Value)> =
        r_events.iter().filter(|(e, _)| e != "ping").collect();
    let n_non_ping: Vec<&(String, Value)> =
        n_events.iter().filter(|(e, _)| e != "ping").collect();

    // Esperado: 2x msg + 1 ready
    assert!(
        r_non_ping.len() >= 3,
        "rust events: {:?}",
        r_non_ping
    );
    assert!(
        n_non_ping.len() >= 3,
        "node events: {:?}",
        n_non_ping
    );
    assert_eq!(r_non_ping[0].0, "msg");
    assert_eq!(n_non_ping[0].0, "msg");
    assert_eq!(r_non_ping[0].1["idx"], 0);
    assert_eq!(n_non_ping[0].1["idx"], 0);
    assert_eq!(r_non_ping[1].0, "msg");
    assert_eq!(n_non_ping[1].0, "msg");
    assert_eq!(r_non_ping[1].1["idx"], 1);
    assert_eq!(n_non_ping[1].1["idx"], 1);
    assert_eq!(r_non_ping[2].0, "ready");
    assert_eq!(n_non_ping[2].0, "ready");
    assert_eq!(r_non_ping[2].1["idx"], 2);
    assert_eq!(n_non_ping[2].1["idx"], 2);
    assert_eq!(r_non_ping[2].1["agent"], "x");
    assert_eq!(n_non_ping[2].1["agent"], "x");
}

#[test]
fn stream_since_idx_skips_old() {
    let Some((_tr, _tn, mr, mn, sr, sn)) = both_servers() else {
        return;
    };
    let entries = vec![
        json!({ "role": "user", "content": "oi" }),
        json!({ "role": "agent", "content": "tudo bem?" }),
        json!({ "role": "user", "content": "ok" }),
    ];
    seed_chat(&mr, "y", &entries);
    seed_chat(&mn, "y", &entries);
    let r_raw = curl_sse(&rurl(sr.port, "/api/chat/stream?agent=y&since_idx=2"), 1);
    let n_raw = curl_sse(&rurl(sn.port, "/api/chat/stream?agent=y&since_idx=2"), 1);
    let r_events = parse_sse(&r_raw);
    let n_events = parse_sse(&n_raw);
    let r_non_ping: Vec<&(String, Value)> =
        r_events.iter().filter(|(e, _)| e != "ping").collect();
    let n_non_ping: Vec<&(String, Value)> =
        n_events.iter().filter(|(e, _)| e != "ping").collect();
    // Esperado: só 1 msg (idx 2) + 1 ready (idx 3)
    assert_eq!(r_non_ping.len(), 2);
    assert_eq!(n_non_ping.len(), 2);
    assert_eq!(r_non_ping[0].0, "msg");
    assert_eq!(r_non_ping[0].1["idx"], 2);
    assert_eq!(n_non_ping[0].0, "msg");
    assert_eq!(n_non_ping[0].1["idx"], 2);
    assert_eq!(r_non_ping[1].0, "ready");
    assert_eq!(r_non_ping[1].1["idx"], 3);
    assert_eq!(n_non_ping[1].0, "ready");
    assert_eq!(n_non_ping[1].1["idx"], 3);
}

#[test]
fn stream_no_file_empty_snapshot_with_ready() {
    let Some((_tr, _tn, _mr, _mn, sr, sn)) = both_servers() else {
        return;
    };
    let r_raw = curl_sse(&rurl(sr.port, "/api/chat/stream?agent=never"), 1);
    let n_raw = curl_sse(&rurl(sn.port, "/api/chat/stream?agent=never"), 1);
    let r_events = parse_sse(&r_raw);
    let n_events = parse_sse(&n_raw);
    let r_non_ping: Vec<&(String, Value)> =
        r_events.iter().filter(|(e, _)| e != "ping").collect();
    let n_non_ping: Vec<&(String, Value)> =
        n_events.iter().filter(|(e, _)| e != "ping").collect();
    assert_eq!(r_non_ping.len(), 1);
    assert_eq!(n_non_ping.len(), 1);
    assert_eq!(r_non_ping[0].0, "ready");
    assert_eq!(n_non_ping[0].0, "ready");
    assert_eq!(r_non_ping[0].1["idx"], 0);
    assert_eq!(n_non_ping[0].1["idx"], 0);
}
