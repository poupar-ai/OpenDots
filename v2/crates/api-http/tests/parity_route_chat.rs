//! Parity tests — F4-05f-2: `POST /api/agents/route` + `GET /api/chat/history`.
//!
//! Rotas cobertas:
//! - `POST /api/agents/route`      — router.route wrapped (paridade `handleRoute`)
//! - `GET  /api/chat/history?agent=X` — JSONL reader (paridade `handleChatHistory`)

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

fn seed_graph(mem_dir: &Path, g: &Value) {
    std::fs::write(
        mem_dir.join("graph.json"),
        serde_json::to_string_pretty(g).unwrap(),
    )
    .unwrap();
}

// ── POST /api/agents/route ──────────────────────────────────────────────────

#[test]
fn route_invalid_json_400() {
    let Some((_tr, _tn, _mr, _mn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl("POST", &rurl(sr.port, "/api/agents/route"), Some("not-json"));
    let n = curl("POST", &rurl(sn.port, "/api/agents/route"), Some("not-json"));
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body, json!({ "error": "invalid json" }));
}

#[test]
fn route_missing_from_400() {
    let Some((_tr, _tn, _mr, _mn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl("POST", &rurl(sr.port, "/api/agents/route"), Some("{}"));
    let n = curl("POST", &rurl(sn.port, "/api/agents/route"), Some("{}"));
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body, json!({ "error": "need {from}" }));
}

#[test]
fn route_invalid_intent_400() {
    let Some((_tr, _tn, _mr, _mn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"from":"a","intent":"foo"}"#;
    let r = curl("POST", &rurl(sr.port, "/api/agents/route"), Some(body));
    let n = curl("POST", &rurl(sn.port, "/api/agents/route"), Some(body));
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body["error"], n.body["error"]);
    assert_eq!(r.body["error"], "invalid intent");
    // Allowed list: preserve order (serde_json preserve_order)
    assert_eq!(r.body["allowed"], n.body["allowed"]);
}

#[test]
fn route_unknown_agent_404() {
    let Some((_tr, _tn, mr, mn, sr, sn)) = both_servers() else {
        return;
    };
    let manifest = json!({ "version": 1, "manager": "m", "oracle": "o", "agents": {} });
    seed_manifest(&mr, &manifest);
    seed_manifest(&mn, &manifest);
    let body = r#"{"from":"ghost"}"#;
    let r = curl("POST", &rurl(sr.port, "/api/agents/route"), Some(body));
    let n = curl("POST", &rurl(sn.port, "/api/agents/route"), Some(body));
    assert_eq!(r.status, 404);
    assert_eq!(n.status, 404);
    assert_eq!(r.body, n.body);
}

#[test]
fn route_ok_with_can_talk_to() {
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
    let body = r#"{"from":"a"}"#;
    let r = curl("POST", &rurl(sr.port, "/api/agents/route"), Some(body));
    let n = curl("POST", &rurl(sn.port, "/api/agents/route"), Some(body));
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    assert_eq!(r.body["from"], n.body["from"]);
    assert_eq!(r.body["can_talk_to"], n.body["can_talk_to"]);
    assert_eq!(r.body["note"], n.body["note"]);
    let r_cands: Vec<&str> = r.body["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["agent"].as_str())
        .collect();
    let n_cands: Vec<&str> = n.body["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["agent"].as_str())
        .collect();
    assert_eq!(r_cands, n_cands);
}

#[test]
fn route_escalate_prioritizes_firefighter() {
    let Some((_tr, _tn, mr, mn, sr, sn)) = both_servers() else {
        return;
    };
    let manifest = json!({
        "version": 1, "manager": "m", "oracle": "o",
        "agents": {
            "a": { "type": "specialist", "can_talk_to": ["b", "internal_firefighter", "c"] },
            "b": { "type": "specialist" },
            "c": { "type": "specialist" },
            "internal_firefighter": { "type": "firefighter" },
        }
    });
    seed_manifest(&mr, &manifest);
    seed_manifest(&mn, &manifest);
    let body = r#"{"from":"a","intent":"escalate"}"#;
    let r = curl("POST", &rurl(sr.port, "/api/agents/route"), Some(body));
    let n = curl("POST", &rurl(sn.port, "/api/agents/route"), Some(body));
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    let r_cands: Vec<&str> = r.body["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["agent"].as_str())
        .collect();
    let n_cands: Vec<&str> = n.body["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["agent"].as_str())
        .collect();
    assert_eq!(r_cands, n_cands);
    assert_eq!(r_cands[0], "internal_firefighter");
}

#[test]
fn route_with_file_boosts_editor() {
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
    let graph = json!({
        "version": 2,
        "nodes": {},
        "edges": {
            "e1": { "rel": "edits", "from": "agent:c", "to": "file:x.rs", "count": 10, "last_seen": "2026-04-19T00:00:00.000Z" }
        },
        "updated_at": "2026-04-19T00:00:00.000Z"
    });
    seed_graph(&mr, &graph);
    seed_graph(&mn, &graph);
    let body = r#"{"from":"a","file":"x.rs"}"#;
    let r = curl("POST", &rurl(sr.port, "/api/agents/route"), Some(body));
    let n = curl("POST", &rurl(sn.port, "/api/agents/route"), Some(body));
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    let r_first = r.body["candidates"][0]["agent"].as_str().unwrap();
    let n_first = n.body["candidates"][0]["agent"].as_str().unwrap();
    assert_eq!(r_first, n_first);
    assert_eq!(r_first, "c");
}

// ── GET /api/chat/history ───────────────────────────────────────────────────

#[test]
fn chat_history_missing_agent_400() {
    let Some((_tr, _tn, _mr, _mn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl("GET", &rurl(sr.port, "/api/chat/history"), None);
    let n = curl("GET", &rurl(sn.port, "/api/chat/history"), None);
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body, json!({ "error": "missing agent" }));
}

#[test]
fn chat_history_no_file_empty() {
    let Some((_tr, _tn, _mr, _mn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl("GET", &rurl(sr.port, "/api/chat/history?agent=nope"), None);
    let n = curl("GET", &rurl(sn.port, "/api/chat/history?agent=nope"), None);
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body, json!({ "agent": "nope", "messages": [] }));
}

#[test]
fn chat_history_with_messages() {
    let Some((_tr, _tn, mr, mn, sr, sn)) = both_servers() else {
        return;
    };
    // Seed both mem dirs with same JSONL content
    let chat_r = mr.join("agent_chats");
    let chat_n = mn.join("agent_chats");
    std::fs::create_dir_all(&chat_r).unwrap();
    std::fs::create_dir_all(&chat_n).unwrap();
    let lines = "{\"role\":\"user\",\"text\":\"oi\"}\n{\"role\":\"agent\",\"text\":\"tudo bem?\"}\n";
    std::fs::write(chat_r.join("alice.jsonl"), lines).unwrap();
    std::fs::write(chat_n.join("alice.jsonl"), lines).unwrap();
    let r = curl("GET", &rurl(sr.port, "/api/chat/history?agent=alice"), None);
    let n = curl("GET", &rurl(sn.port, "/api/chat/history?agent=alice"), None);
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body["agent"], "alice");
    assert_eq!(r.body["messages"].as_array().unwrap().len(), 2);
}

#[test]
fn chat_history_skips_invalid_lines() {
    let Some((_tr, _tn, mr, mn, sr, sn)) = both_servers() else {
        return;
    };
    let chat_r = mr.join("agent_chats");
    let chat_n = mn.join("agent_chats");
    std::fs::create_dir_all(&chat_r).unwrap();
    std::fs::create_dir_all(&chat_n).unwrap();
    let lines = "{\"role\":\"user\",\"text\":\"oi\"}\nnot-json\n\n{\"role\":\"agent\",\"text\":\"ok\"}\n";
    std::fs::write(chat_r.join("bob.jsonl"), lines).unwrap();
    std::fs::write(chat_n.join("bob.jsonl"), lines).unwrap();
    let r = curl("GET", &rurl(sr.port, "/api/chat/history?agent=bob"), None);
    let n = curl("GET", &rurl(sn.port, "/api/chat/history?agent=bob"), None);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body["messages"].as_array().unwrap().len(), 2);
}
