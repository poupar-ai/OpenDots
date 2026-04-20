//! Parity tests — `POST /api/agents/handoff` (F4-05f-3).
//!
//! Rota complexa (185 linhas no .cjs). Cobre:
//! - Validação body (invalid json, missing fields, invalid intent)
//! - 404 from/to unknown + suggested_targets
//! - 403 can_talk_to violation
//! - 403 destructive intent (sem bombeiro, com bombeiro)
//! - Happy path: chat append, log append, bus emit
//!
//! **Não testa** spawn Claude ou drain de fila (runner é no-op até F4-05f-5).

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

#[derive(Debug)]
struct Resp {
    status: u16,
    body: Value,
}

fn curl(method: &str, url: &str, body: Option<&str>) -> Resp {
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
    Resp { status, body }
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

fn seed_manifest(mem_dir: &Path, manifest: &Value) {
    std::fs::write(
        mem_dir.join("agents.json"),
        serde_json::to_string_pretty(manifest).unwrap(),
    )
    .unwrap();
}

fn read_jsonl(path: &Path) -> Vec<Value> {
    let raw = std::fs::read_to_string(path).unwrap_or_default();
    raw.split('\n')
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect()
}

// ── Validation ──────────────────────────────────────────────────────────────

#[test]
fn handoff_invalid_json_400() {
    let Some((_tr, _tn, _mr, _mn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl(
        "POST",
        &rurl(sr.port, "/api/agents/handoff"),
        Some("not-json"),
    );
    let n = curl(
        "POST",
        &rurl(sn.port, "/api/agents/handoff"),
        Some("not-json"),
    );
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body, json!({ "error": "invalid json" }));
}

#[test]
fn handoff_missing_fields_400() {
    let Some((_tr, _tn, _mr, _mn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"from":"a"}"#;
    let r = curl("POST", &rurl(sr.port, "/api/agents/handoff"), Some(body));
    let n = curl("POST", &rurl(sn.port, "/api/agents/handoff"), Some(body));
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body, json!({ "error": "need {from, to, message}" }));
}

#[test]
fn handoff_invalid_intent_400() {
    let Some((_tr, _tn, mr, mn, sr, sn)) = both_servers() else {
        return;
    };
    let manifest = json!({
        "version": 1, "manager": "m", "oracle": "o",
        "agents": {
            "a": { "type": "specialist", "can_talk_to": ["b"] },
            "b": { "type": "specialist" },
        }
    });
    seed_manifest(&mr, &manifest);
    seed_manifest(&mn, &manifest);
    let body = r#"{"from":"a","to":"b","message":"oi","intent":"foo"}"#;
    let r = curl("POST", &rurl(sr.port, "/api/agents/handoff"), Some(body));
    let n = curl("POST", &rurl(sn.port, "/api/agents/handoff"), Some(body));
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body["error"], n.body["error"]);
    assert_eq!(r.body["got"], n.body["got"]);
    assert_eq!(r.body["allowed"], n.body["allowed"]);
    assert_eq!(r.body["error"], "invalid intent");
}

// ── 404 from/to unknown ─────────────────────────────────────────────────────

#[test]
fn handoff_unknown_from_404() {
    let Some((_tr, _tn, mr, mn, sr, sn)) = both_servers() else {
        return;
    };
    let manifest = json!({
        "version": 1, "manager": "m", "oracle": "o",
        "agents": { "b": { "type": "specialist" } }
    });
    seed_manifest(&mr, &manifest);
    seed_manifest(&mn, &manifest);
    let body = r#"{"from":"ghost","to":"b","message":"oi"}"#;
    let r = curl("POST", &rurl(sr.port, "/api/agents/handoff"), Some(body));
    let n = curl("POST", &rurl(sn.port, "/api/agents/handoff"), Some(body));
    assert_eq!(r.status, 404);
    assert_eq!(n.status, 404);
    assert_eq!(r.body, n.body);
}

#[test]
fn handoff_unknown_to_404_with_suggestions() {
    let Some((_tr, _tn, mr, mn, sr, sn)) = both_servers() else {
        return;
    };
    let manifest = json!({
        "version": 1, "manager": "m", "oracle": "o",
        "agents": {
            "a": { "type": "specialist", "can_talk_to": ["b", "c"] },
            "b": { "type": "specialist" },
            "c": { "type": "specialist" },
        }
    });
    seed_manifest(&mr, &manifest);
    seed_manifest(&mn, &manifest);
    let body = r#"{"from":"a","to":"nope","message":"oi"}"#;
    let r = curl("POST", &rurl(sr.port, "/api/agents/handoff"), Some(body));
    let n = curl("POST", &rurl(sn.port, "/api/agents/handoff"), Some(body));
    assert_eq!(r.status, 404);
    assert_eq!(n.status, 404);
    assert_eq!(r.body["error"], n.body["error"]);
    assert_eq!(r.body["file_detected"], n.body["file_detected"]);
    // suggestions count can match
    let r_sugs = r.body["suggested_targets"].as_array().unwrap();
    let n_sugs = n.body["suggested_targets"].as_array().unwrap();
    assert_eq!(r_sugs.len(), n_sugs.len());
}

// ── 403 can_talk_to violation ───────────────────────────────────────────────

#[test]
fn handoff_forbidden_not_in_can_talk_to_403() {
    let Some((_tr, _tn, mr, mn, sr, sn)) = both_servers() else {
        return;
    };
    let manifest = json!({
        "version": 1, "manager": "m", "oracle": "o",
        "agents": {
            "a": { "type": "specialist", "can_talk_to": ["b"] },
            "b": { "type": "specialist" },
            "c": { "type": "specialist" },
        }
    });
    seed_manifest(&mr, &manifest);
    seed_manifest(&mn, &manifest);
    let body = r#"{"from":"a","to":"c","message":"oi"}"#;
    let r = curl("POST", &rurl(sr.port, "/api/agents/handoff"), Some(body));
    let n = curl("POST", &rurl(sn.port, "/api/agents/handoff"), Some(body));
    assert_eq!(r.status, 403);
    assert_eq!(n.status, 403);
    assert_eq!(r.body["error"], n.body["error"]);
    assert_eq!(r.body["reason"], n.body["reason"]);
    assert_eq!(r.body["allowed"], n.body["allowed"]);
    assert_eq!(r.body["error"], "forbidden");
    // violation logged
    let log_r = read_jsonl(&mr.join("agent_logs/a.jsonl"));
    let log_n = read_jsonl(&mn.join("agent_logs/a.jsonl"));
    assert_eq!(log_r.len(), 1);
    assert_eq!(log_n.len(), 1);
    assert_eq!(log_r[0]["type"], "violation");
    assert_eq!(log_n[0]["type"], "violation");
    assert_eq!(log_r[0]["stage"], log_n[0]["stage"]);
    assert_eq!(log_r[0]["target"], log_n[0]["target"]);
}

// ── 200 OK happy path ───────────────────────────────────────────────────────

#[test]
fn handoff_ok_writes_chat_and_logs() {
    let Some((_tr, _tn, mr, mn, sr, sn)) = both_servers() else {
        return;
    };
    let manifest = json!({
        "version": 1, "manager": "m", "oracle": "o",
        "agents": {
            "a": { "type": "specialist", "can_talk_to": ["b"] },
            "b": { "type": "specialist" },
        }
    });
    seed_manifest(&mr, &manifest);
    seed_manifest(&mn, &manifest);
    let body = r#"{"from":"a","to":"b","message":"oi b"}"#;
    let r = curl("POST", &rurl(sr.port, "/api/agents/handoff"), Some(body));
    let n = curl("POST", &rurl(sn.port, "/api/agents/handoff"), Some(body));
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    assert_eq!(r.body["ok"], n.body["ok"]);
    assert_eq!(r.body["to"], n.body["to"]);
    assert_eq!(r.body["intent"], n.body["intent"]);
    assert_eq!(r.body["auto_queued"], n.body["auto_queued"]);
    assert_eq!(r.body["auto_resumed"], n.body["auto_resumed"]);
    assert_eq!(r.body["auto_escalated"], n.body["auto_escalated"]);
    assert_eq!(r.body["intent"], "ask");

    // Chat de "b" recebeu 1 entrada
    let chat_r = read_jsonl(&mr.join("agent_chats/b.jsonl"));
    let chat_n = read_jsonl(&mn.join("agent_chats/b.jsonl"));
    assert_eq!(chat_r.len(), 1);
    assert_eq!(chat_n.len(), 1);
    assert_eq!(chat_r[0]["role"], "user");
    assert_eq!(chat_r[0]["content"], "oi b");
    assert_eq!(chat_r[0]["from"], "a");
    assert_eq!(chat_r[0]["intent"], "ask");
    assert_eq!(chat_r[0]["role"], chat_n[0]["role"]);
    assert_eq!(chat_r[0]["content"], chat_n[0]["content"]);
    assert_eq!(chat_r[0]["from"], chat_n[0]["from"]);

    // Logs de "a" (handoff_out) e "b" (handoff_in)
    let log_a_r = read_jsonl(&mr.join("agent_logs/a.jsonl"));
    let log_a_n = read_jsonl(&mn.join("agent_logs/a.jsonl"));
    assert_eq!(log_a_r.len(), 1);
    assert_eq!(log_a_n.len(), 1);
    assert_eq!(log_a_r[0]["type"], "handoff_out");
    assert_eq!(log_a_n[0]["type"], "handoff_out");
    assert_eq!(log_a_r[0]["chars"], 4); // "oi b".len()
    assert_eq!(log_a_n[0]["chars"], 4);
    assert_eq!(log_a_r[0]["target"], log_a_n[0]["target"]);

    let log_b_r = read_jsonl(&mr.join("agent_logs/b.jsonl"));
    let log_b_n = read_jsonl(&mn.join("agent_logs/b.jsonl"));
    assert_eq!(log_b_r.len(), 1);
    assert_eq!(log_b_n.len(), 1);
    assert_eq!(log_b_r[0]["type"], "handoff_in");
    assert_eq!(log_b_n[0]["type"], "handoff_in");
    assert_eq!(log_b_r[0]["source"], "a");
    assert_eq!(log_b_n[0]["source"], "a");
}

// ── 403 destructive intent ──────────────────────────────────────────────────

#[test]
fn handoff_destructive_intent_blocks_403() {
    let Some((_tr, _tn, mr, mn, sr, sn)) = both_servers() else {
        return;
    };
    let manifest = json!({
        "version": 1, "manager": "m", "oracle": "o",
        "agents": {
            "a": { "type": "specialist", "can_talk_to": ["b", "internal_firefighter"] },
            "b": { "type": "specialist" },
            "internal_firefighter": { "type": "firefighter" },
        }
    });
    seed_manifest(&mr, &manifest);
    seed_manifest(&mn, &manifest);
    // "rm -rf /" é padrão core destrutivo
    let body = r#"{"from":"a","to":"b","message":"por favor rm -rf / agora"}"#;
    let r = curl("POST", &rurl(sr.port, "/api/agents/handoff"), Some(body));
    let n = curl("POST", &rurl(sn.port, "/api/agents/handoff"), Some(body));
    assert_eq!(r.status, 403);
    assert_eq!(n.status, 403);
    assert_eq!(r.body["error"], n.body["error"]);
    assert_eq!(r.body["reason"], n.body["reason"]);
    assert_eq!(r.body["escalated_to"], n.body["escalated_to"]);
    assert_eq!(r.body["error"], "blocked");
    assert_eq!(r.body["reason"], "destructive_intent");
    assert_eq!(r.body["escalated_to"], "internal_firefighter");
}
