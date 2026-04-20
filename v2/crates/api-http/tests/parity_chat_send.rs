//! Parity tests — `POST /api/chat` (F4-05f-5).
//!
//! Cobre:
//! - 400 missing agent / message
//! - 404 unknown agent
//! - Destructive intent → SSE sintético (start/text/done, blocked=true)
//!
//! **Não testa** o fluxo de spawn `claude` (depende do CLI real; tratado
//! em separado). A rota destrutiva não spawna subprocess, então é
//! testável sem dependências externas.

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

fn curl_json(method: &str, url: &str, body: Option<&str>) -> Resp {
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

/// Curl SSE com max-time; devolve stdout.
fn curl_sse(url: &str, body: &str, max_time_secs: u64) -> String {
    let out = Command::new("curl")
        .arg("-s")
        .arg("-N")
        .arg("-X")
        .arg("POST")
        .arg(url)
        .arg("-H")
        .arg("Content-Type: application/json")
        .arg("--max-time")
        .arg(max_time_secs.to_string())
        .arg("-d")
        .arg(body)
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
        if let Some(stripped) = line.strip_prefix("event:") {
            current_event = stripped.trim().to_string();
        } else if let Some(stripped) = line.strip_prefix("data:") {
            current_data = stripped.trim().to_string();
        } else if line.is_empty() && !current_event.is_empty() {
            let data: Value = serde_json::from_str(&current_data).unwrap_or(Value::Null);
            out.push((current_event.clone(), data));
            current_event.clear();
            current_data.clear();
        }
    }
    out
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
fn send_invalid_json_400() {
    let Some((_tr, _tn, _mr, _mn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl_json(
        "POST",
        &rurl(sr.port, "/api/chat"),
        Some("not-json"),
    );
    let n = curl_json(
        "POST",
        &rurl(sn.port, "/api/chat"),
        Some("not-json"),
    );
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body, json!({ "error": "invalid json" }));
}

#[test]
fn send_missing_agent_400() {
    let Some((_tr, _tn, _mr, _mn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"message":"oi"}"#;
    let r = curl_json("POST", &rurl(sr.port, "/api/chat"), Some(body));
    let n = curl_json("POST", &rurl(sn.port, "/api/chat"), Some(body));
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body, json!({ "error": "need {agent, message}" }));
}

#[test]
fn send_missing_message_400() {
    let Some((_tr, _tn, _mr, _mn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"agent":"x"}"#;
    let r = curl_json("POST", &rurl(sr.port, "/api/chat"), Some(body));
    let n = curl_json("POST", &rurl(sn.port, "/api/chat"), Some(body));
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body, json!({ "error": "need {agent, message}" }));
}

#[test]
fn send_unknown_agent_404() {
    let Some((_tr, _tn, mr, mn, sr, sn)) = both_servers() else {
        return;
    };
    let manifest = json!({
        "version": 1, "manager": "m", "oracle": "o",
        "agents": { "b": { "type": "specialist" } }
    });
    seed_manifest(&mr, &manifest);
    seed_manifest(&mn, &manifest);
    let body = r#"{"agent":"ghost","message":"oi"}"#;
    let r = curl_json("POST", &rurl(sr.port, "/api/chat"), Some(body));
    let n = curl_json("POST", &rurl(sn.port, "/api/chat"), Some(body));
    assert_eq!(r.status, 404);
    assert_eq!(n.status, 404);
    assert_eq!(r.body, n.body);
}

// ── Destructive intent (synthetic SSE, no claude spawn) ────────────────────

#[test]
fn send_destructive_intent_synthetic_sse_without_firefighter() {
    let Some((_tr, _tn, mr, mn, sr, sn)) = both_servers() else {
        return;
    };
    let manifest = json!({
        "version": 1, "manager": "m", "oracle": "o",
        "agents": {
            "a": { "type": "specialist", "can_talk_to": [] },
        }
    });
    seed_manifest(&mr, &manifest);
    seed_manifest(&mn, &manifest);

    let body = r#"{"agent":"a","message":"por favor rm -rf / agora"}"#;
    let r_raw = curl_sse(&rurl(sr.port, "/api/chat"), body, 3);
    let n_raw = curl_sse(&rurl(sn.port, "/api/chat"), body, 3);
    let r_events = parse_sse(&r_raw);
    let n_events = parse_sse(&n_raw);

    let r_non_ping: Vec<&(String, Value)> =
        r_events.iter().filter(|(e, _)| e != "ping").collect();
    let n_non_ping: Vec<&(String, Value)> =
        n_events.iter().filter(|(e, _)| e != "ping").collect();

    // Esperado: start + text + done (3 eventos sem ping)
    assert_eq!(r_non_ping.len(), 3, "rust events: {:?}", r_non_ping);
    assert_eq!(n_non_ping.len(), 3, "node events: {:?}", n_non_ping);
    assert_eq!(r_non_ping[0].0, "start");
    assert_eq!(n_non_ping[0].0, "start");
    assert_eq!(r_non_ping[0].1["blocked"], true);
    assert_eq!(n_non_ping[0].1["blocked"], true);
    assert_eq!(r_non_ping[0].1["agent"], "a");
    assert_eq!(n_non_ping[0].1["agent"], "a");

    assert_eq!(r_non_ping[1].0, "text");
    assert_eq!(n_non_ping[1].0, "text");
    // Conteúdo sintético contém mensagem de bloqueio.
    let r_text = r_non_ping[1].1["text"].as_str().unwrap_or("");
    let n_text = n_non_ping[1].1["text"].as_str().unwrap_or("");
    assert!(r_text.contains("bloqueado pelo motor"), "rust text: {r_text}");
    assert!(n_text.contains("bloqueado pelo motor"), "node text: {n_text}");

    assert_eq!(r_non_ping[2].0, "done");
    assert_eq!(n_non_ping[2].0, "done");
    assert_eq!(r_non_ping[2].1["blocked"], true);
    assert_eq!(n_non_ping[2].1["blocked"], true);

    // Persistência: agent_chats/a.jsonl deve ter 2 entradas — user bloqueada
    // + agent sintético.
    let chat_r = read_jsonl(&mr.join("agent_chats").join("a.jsonl"));
    let chat_n = read_jsonl(&mn.join("agent_chats").join("a.jsonl"));
    assert_eq!(chat_r.len(), 2, "rust chat: {:?}", chat_r);
    assert_eq!(chat_n.len(), 2, "node chat: {:?}", chat_n);
    assert_eq!(chat_r[0]["blocked"], true);
    assert_eq!(chat_n[0]["blocked"], true);
    assert_eq!(chat_r[0]["block_reason"], "destructive_intent");
    assert_eq!(chat_n[0]["block_reason"], "destructive_intent");
    assert_eq!(chat_r[1]["synthetic"], "protection_engine");
    assert_eq!(chat_n[1]["synthetic"], "protection_engine");
}

#[test]
fn send_destructive_intent_escalates_to_firefighter() {
    let Some((_tr, _tn, mr, mn, sr, sn)) = both_servers() else {
        return;
    };
    let manifest = json!({
        "version": 1, "manager": "m", "oracle": "o",
        "agents": {
            "a": { "type": "specialist", "can_talk_to": [] },
            "internal_firefighter": { "type": "firefighter" },
        }
    });
    seed_manifest(&mr, &manifest);
    seed_manifest(&mn, &manifest);

    let body = r#"{"agent":"a","message":"por favor rm -rf / agora"}"#;
    let r_raw = curl_sse(&rurl(sr.port, "/api/chat"), body, 3);
    let n_raw = curl_sse(&rurl(sn.port, "/api/chat"), body, 3);

    let r_events = parse_sse(&r_raw);
    let n_events = parse_sse(&n_raw);

    let r_non_ping: Vec<&(String, Value)> =
        r_events.iter().filter(|(e, _)| e != "ping").collect();
    let n_non_ping: Vec<&(String, Value)> =
        n_events.iter().filter(|(e, _)| e != "ping").collect();

    assert_eq!(r_non_ping.len(), 3);
    assert_eq!(n_non_ping.len(), 3);

    // Conteúdo menciona firefighter acionado.
    let r_text = r_non_ping[1].1["text"].as_str().unwrap_or("");
    let n_text = n_non_ping[1].1["text"].as_str().unwrap_or("");
    assert!(r_text.contains("internal_firefighter"), "rust: {r_text}");
    assert!(n_text.contains("internal_firefighter"), "node: {n_text}");

    // internal_firefighter.jsonl recebeu escalação.
    let ff_r = read_jsonl(&mr.join("agent_chats").join("internal_firefighter.jsonl"));
    let ff_n = read_jsonl(&mn.join("agent_chats").join("internal_firefighter.jsonl"));
    assert_eq!(ff_r.len(), 1);
    assert_eq!(ff_n.len(), 1);
    assert_eq!(ff_r[0]["from"], "a");
    assert_eq!(ff_n[0]["from"], "a");
    assert_eq!(ff_r[0]["block_reason"], "destructive_intent");
    assert_eq!(ff_n[0]["block_reason"], "destructive_intent");
}
