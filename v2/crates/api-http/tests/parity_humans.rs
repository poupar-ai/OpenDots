//! Parity tests — `api-http` humans (F4-05d-1) vs `plugin/ui/server.cjs`.
//!
//! 6 rotas:
//! - `GET    /api/humans`
//! - `POST   /api/humans`
//! - `GET    /api/humans/:id`
//! - `PUT    /api/humans/:id`
//! - `DELETE /api/humans/:id`
//! - `POST   /api/humans/:id/heartbeat`

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};
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

/// Remove campos voláteis (timestamps) que diferem entre runs paralelas.
fn strip_volatile(v: &mut Value) {
    match v {
        Value::Object(m) => {
            m.remove("created_at");
            m.remove("last_seen_at");
            // `online` depende do clock — tira também para comparar shape.
            m.remove("online");
            for (_, child) in m.iter_mut() {
                strip_volatile(child);
            }
        }
        Value::Array(a) => {
            for x in a {
                strip_volatile(x);
            }
        }
        _ => {}
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[test]
fn list_empty_parity() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl("GET", &rurl(sr.port, "/api/humans"), None);
    let n = curl("GET", &rurl(sn.port, "/api/humans"), None);
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body, json!({ "humans": [], "default_id": Value::Null }));
}

#[test]
fn create_missing_name_400() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{}"#;
    let r = curl("POST", &rurl(sr.port, "/api/humans"), Some(body));
    let n = curl("POST", &rurl(sn.port, "/api/humans"), Some(body));
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body, json!({ "error": "need name" }));
}

#[test]
fn create_invalid_json_400() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = "not-json";
    let r = curl("POST", &rurl(sr.port, "/api/humans"), Some(body));
    let n = curl("POST", &rurl(sn.port, "/api/humans"), Some(body));
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body, json!({ "error": "invalid json" }));
}

#[test]
fn create_empty_name_whitespace_400() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"name":"   "}"#;
    let r = curl("POST", &rurl(sr.port, "/api/humans"), Some(body));
    let n = curl("POST", &rurl(sn.port, "/api/humans"), Some(body));
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
}

#[test]
fn create_ok_shape_parity() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"name":"Alice","description":"hi","skills":["rust","ts"]}"#;
    let mut r = curl("POST", &rurl(sr.port, "/api/humans"), Some(body));
    let mut n = curl("POST", &rurl(sn.port, "/api/humans"), Some(body));
    assert_eq!(r.status, 201);
    assert_eq!(n.status, 201);
    strip_volatile(&mut r.body);
    strip_volatile(&mut n.body);
    assert_eq!(r.body, n.body);
    // Check key shape:
    let h = r.body.get("human").unwrap();
    assert_eq!(h.get("id").unwrap(), "alice");
    assert_eq!(h.get("name").unwrap(), "Alice");
    assert_eq!(h.get("skills").unwrap(), &json!(["rust", "ts"]));
}

#[test]
fn create_sets_default_id_when_first() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"name":"Bob"}"#;
    let _ = curl("POST", &rurl(sr.port, "/api/humans"), Some(body));
    let _ = curl("POST", &rurl(sn.port, "/api/humans"), Some(body));
    let r = curl("GET", &rurl(sr.port, "/api/humans"), None);
    let n = curl("GET", &rurl(sn.port, "/api/humans"), None);
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    assert_eq!(r.body.get("default_id").unwrap(), "bob");
    assert_eq!(n.body.get("default_id").unwrap(), "bob");
}

#[test]
fn create_collision_appends_suffix_parity() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"name":"Carol"}"#;
    let r1 = curl("POST", &rurl(sr.port, "/api/humans"), Some(body));
    let n1 = curl("POST", &rurl(sn.port, "/api/humans"), Some(body));
    let r2 = curl("POST", &rurl(sr.port, "/api/humans"), Some(body));
    let n2 = curl("POST", &rurl(sn.port, "/api/humans"), Some(body));
    assert_eq!(r1.body["human"]["id"], "carol");
    assert_eq!(n1.body["human"]["id"], "carol");
    assert_eq!(r2.body["human"]["id"], "carol-2");
    assert_eq!(n2.body["human"]["id"], "carol-2");
}

#[test]
fn get_unknown_404_parity() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl("GET", &rurl(sr.port, "/api/humans/nonexistent"), None);
    let n = curl("GET", &rurl(sn.port, "/api/humans/nonexistent"), None);
    assert_eq!(r.status, 404);
    assert_eq!(n.status, 404);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body, json!({ "error": "not found" }));
}

#[test]
fn get_known_after_create_parity() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"name":"Dan","location":"devs","gender":"male"}"#;
    let _ = curl("POST", &rurl(sr.port, "/api/humans"), Some(body));
    let _ = curl("POST", &rurl(sn.port, "/api/humans"), Some(body));
    let mut r = curl("GET", &rurl(sr.port, "/api/humans/dan"), None);
    let mut n = curl("GET", &rurl(sn.port, "/api/humans/dan"), None);
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    strip_volatile(&mut r.body);
    strip_volatile(&mut n.body);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body["human"]["location"], "devs");
    assert_eq!(r.body["human"]["gender"], "male");
}

#[test]
fn sanitize_drops_invalid_location_parity() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"name":"Eve","location":"invalidloc","gender":"weird"}"#;
    let mut r = curl("POST", &rurl(sr.port, "/api/humans"), Some(body));
    let mut n = curl("POST", &rurl(sn.port, "/api/humans"), Some(body));
    assert_eq!(r.status, 201);
    assert_eq!(n.status, 201);
    strip_volatile(&mut r.body);
    strip_volatile(&mut n.body);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body["human"]["location"], Value::Null);
    assert_eq!(r.body["human"]["gender"], Value::Null);
}

#[test]
fn update_unknown_404_parity() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"description":"x"}"#;
    let r = curl("PUT", &rurl(sr.port, "/api/humans/ghost"), Some(body));
    let n = curl("PUT", &rurl(sn.port, "/api/humans/ghost"), Some(body));
    assert_eq!(r.status, 404);
    assert_eq!(n.status, 404);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body, json!({ "error": "not found" }));
}

#[test]
fn update_invalid_json_400_parity() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let _ = curl(
        "POST",
        &rurl(sr.port, "/api/humans"),
        Some(r#"{"name":"Fran"}"#),
    );
    let _ = curl(
        "POST",
        &rurl(sn.port, "/api/humans"),
        Some(r#"{"name":"Fran"}"#),
    );
    let body = "not-json";
    let r = curl("PUT", &rurl(sr.port, "/api/humans/fran"), Some(body));
    let n = curl("PUT", &rurl(sn.port, "/api/humans/fran"), Some(body));
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
}

#[test]
fn update_description_parity() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let _ = curl(
        "POST",
        &rurl(sr.port, "/api/humans"),
        Some(r#"{"name":"Gina"}"#),
    );
    let _ = curl(
        "POST",
        &rurl(sn.port, "/api/humans"),
        Some(r#"{"name":"Gina"}"#),
    );
    let body = r#"{"description":"new desc","skills":["a","b"]}"#;
    let mut r = curl("PUT", &rurl(sr.port, "/api/humans/gina"), Some(body));
    let mut n = curl("PUT", &rurl(sn.port, "/api/humans/gina"), Some(body));
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    strip_volatile(&mut r.body);
    strip_volatile(&mut n.body);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body["human"]["description"], "new desc");
    assert_eq!(r.body["human"]["skills"], json!(["a", "b"]));
}

#[test]
fn update_ignores_missing_fields() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let create_body = r#"{"name":"Hank","description":"keep-me"}"#;
    let _ = curl("POST", &rurl(sr.port, "/api/humans"), Some(create_body));
    let _ = curl("POST", &rurl(sn.port, "/api/humans"), Some(create_body));
    // Body vazio → nenhum campo muda.
    let body = r#"{}"#;
    let mut r = curl("PUT", &rurl(sr.port, "/api/humans/hank"), Some(body));
    let mut n = curl("PUT", &rurl(sn.port, "/api/humans/hank"), Some(body));
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    strip_volatile(&mut r.body);
    strip_volatile(&mut n.body);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body["human"]["description"], "keep-me");
}

#[test]
fn heartbeat_unknown_404() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl("POST", &rurl(sr.port, "/api/humans/ghost/heartbeat"), None);
    let n = curl("POST", &rurl(sn.port, "/api/humans/ghost/heartbeat"), None);
    assert_eq!(r.status, 404);
    assert_eq!(n.status, 404);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body, json!({ "error": "not found" }));
}

#[test]
fn heartbeat_ok_shape() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"name":"Ian"}"#;
    let _ = curl("POST", &rurl(sr.port, "/api/humans"), Some(body));
    let _ = curl("POST", &rurl(sn.port, "/api/humans"), Some(body));
    let r = curl("POST", &rurl(sr.port, "/api/humans/ian/heartbeat"), None);
    let n = curl("POST", &rurl(sn.port, "/api/humans/ian/heartbeat"), None);
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    // Strip ts e compare shape.
    let keys_r: Vec<&String> = r
        .body
        .as_object()
        .map(|m| m.keys().collect::<Vec<_>>())
        .unwrap_or_default();
    let mut keys_r: Vec<String> = keys_r.into_iter().cloned().collect();
    keys_r.sort();
    let keys_n: Vec<&String> = n
        .body
        .as_object()
        .map(|m| m.keys().collect::<Vec<_>>())
        .unwrap_or_default();
    let mut keys_n: Vec<String> = keys_n.into_iter().cloned().collect();
    keys_n.sort();
    assert_eq!(keys_r, keys_n);
    assert_eq!(r.body["ok"], true);
    assert_eq!(n.body["ok"], true);
    assert_eq!(r.body["is_online"], true);
    assert_eq!(n.body["is_online"], true);
    assert!(r.body["last_seen_at"].is_string());
    assert!(n.body["last_seen_at"].is_string());
}

#[test]
fn delete_unknown_404() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl("DELETE", &rurl(sr.port, "/api/humans/ghost"), None);
    let n = curl("DELETE", &rurl(sn.port, "/api/humans/ghost"), None);
    assert_eq!(r.status, 404);
    assert_eq!(n.status, 404);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body, json!({ "error": "not found" }));
}

#[test]
fn delete_ok_and_default_id_resets() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"name":"Jane"}"#;
    let _ = curl("POST", &rurl(sr.port, "/api/humans"), Some(body));
    let _ = curl("POST", &rurl(sn.port, "/api/humans"), Some(body));
    let r = curl("DELETE", &rurl(sr.port, "/api/humans/jane"), None);
    let n = curl("DELETE", &rurl(sn.port, "/api/humans/jane"), None);
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body, json!({ "ok": true, "id": "jane" }));
    // default_id deve virar null.
    let lr = curl("GET", &rurl(sr.port, "/api/humans"), None);
    let ln = curl("GET", &rurl(sn.port, "/api/humans"), None);
    assert_eq!(lr.body.get("default_id").unwrap(), &Value::Null);
    assert_eq!(ln.body.get("default_id").unwrap(), &Value::Null);
}

#[test]
fn full_round_trip_parity() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"name":"Kim","description":"first","location":"core","gender":"other"}"#;
    let _ = curl("POST", &rurl(sr.port, "/api/humans"), Some(body));
    let _ = curl("POST", &rurl(sn.port, "/api/humans"), Some(body));

    // List equal (after strip).
    let mut lr = curl("GET", &rurl(sr.port, "/api/humans"), None);
    let mut ln = curl("GET", &rurl(sn.port, "/api/humans"), None);
    strip_volatile(&mut lr.body);
    strip_volatile(&mut ln.body);
    assert_eq!(lr.body, ln.body);

    // Get equal.
    let mut gr = curl("GET", &rurl(sr.port, "/api/humans/kim"), None);
    let mut gn = curl("GET", &rurl(sn.port, "/api/humans/kim"), None);
    strip_volatile(&mut gr.body);
    strip_volatile(&mut gn.body);
    assert_eq!(gr.body, gn.body);

    // Update equal.
    let upd = r#"{"notes":"round trip"}"#;
    let mut ur = curl("PUT", &rurl(sr.port, "/api/humans/kim"), Some(upd));
    let mut un = curl("PUT", &rurl(sn.port, "/api/humans/kim"), Some(upd));
    strip_volatile(&mut ur.body);
    strip_volatile(&mut un.body);
    assert_eq!(ur.body, un.body);

    // Delete equal.
    let dr = curl("DELETE", &rurl(sr.port, "/api/humans/kim"), None);
    let dn = curl("DELETE", &rurl(sn.port, "/api/humans/kim"), None);
    assert_eq!(dr.body, dn.body);
    assert_eq!(dr.body, json!({ "ok": true, "id": "kim" }));
}

// ── F4-05d-2: chat read-only ────────────────────────────────────────────

#[test]
fn chats_summary_empty_parity() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl("GET", &rurl(sr.port, "/api/humans/ghost/chats"), None);
    let n = curl("GET", &rurl(sn.port, "/api/humans/ghost/chats"), None);
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    assert_eq!(r.body, n.body);
    assert_eq!(
        r.body,
        json!({ "human_id": "ghost", "agent_chats": [], "human_chats": [] })
    );
}

#[test]
fn human_chat_history_missing_params_400() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl("GET", &rurl(sr.port, "/api/human-chat/history"), None);
    let n = curl("GET", &rurl(sn.port, "/api/human-chat/history"), None);
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body, json!({ "error": "need me & with" }));
}

#[test]
fn human_chat_history_only_me_400() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl(
        "GET",
        &rurl(sr.port, "/api/human-chat/history?me=alice"),
        None,
    );
    let n = curl(
        "GET",
        &rurl(sn.port, "/api/human-chat/history?me=alice"),
        None,
    );
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
}

#[test]
fn human_chat_history_empty_file_parity() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl(
        "GET",
        &rurl(sr.port, "/api/human-chat/history?me=alice&with=bob"),
        None,
    );
    let n = curl(
        "GET",
        &rurl(sn.port, "/api/human-chat/history?me=alice&with=bob"),
        None,
    );
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    assert_eq!(r.body, n.body);
    assert_eq!(
        r.body,
        json!({ "me": "alice", "with": "bob", "messages": [] })
    );
}

/// Seed: cria um arquivo `human_chats/alice__bob.jsonl` em cada tmpdir
/// com mensagens idênticas, então verifica que `read_human_chat` devolve
/// a mesma coisa em ambos.
#[test]
fn human_chat_history_reads_seeded_file() {
    let Some((tr, tn, sr, sn)) = both_servers() else {
        return;
    };
    for base in [tr.path(), tn.path()] {
        let mem = base.join(".claude-project-memory");
        let d = mem.join("human_chats");
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("alice__bob.jsonl");
        let content = r#"{"ts":"2026-04-19T00:00:00.000Z","from":"alice","content":"hi bob"}
{"ts":"2026-04-19T00:00:01.000Z","from":"bob","content":"hey alice"}
"#;
        std::fs::write(&p, content).unwrap();
    }
    let r = curl(
        "GET",
        &rurl(sr.port, "/api/human-chat/history?me=alice&with=bob"),
        None,
    );
    let n = curl(
        "GET",
        &rurl(sn.port, "/api/human-chat/history?me=alice&with=bob"),
        None,
    );
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    assert_eq!(r.body, n.body);
    assert_eq!(
        r.body["messages"].as_array().map(|a| a.len()),
        Some(2)
    );
    assert_eq!(r.body["messages"][0]["from"], "alice");
    assert_eq!(r.body["messages"][1]["content"], "hey alice");
}

#[test]
fn chats_summary_with_seeded_agent_chat() {
    let Some((tr, tn, sr, sn)) = both_servers() else {
        return;
    };
    // Cria humano `me` em ambos.
    let body = r#"{"name":"Me"}"#;
    let _ = curl("POST", &rurl(sr.port, "/api/humans"), Some(body));
    let _ = curl("POST", &rurl(sn.port, "/api/humans"), Some(body));
    // Seed agent_chats/foo.jsonl em ambos tmpdirs.
    let content = r#"{"role":"user","human_id":"me","ts":"2026-04-19T10:00:00.000Z","content":"olá"}
{"role":"agent","ts":"2026-04-19T10:00:05.000Z","content":"oi"}
"#;
    for base in [tr.path(), tn.path()] {
        let mem = base.join(".claude-project-memory");
        let d = mem.join("agent_chats");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("foo.jsonl"), content).unwrap();
    }
    let r = curl("GET", &rurl(sr.port, "/api/humans/me/chats"), None);
    let n = curl("GET", &rurl(sn.port, "/api/humans/me/chats"), None);
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    assert_eq!(r.body, n.body);
    let ac = r.body["agent_chats"].as_array().unwrap();
    assert_eq!(ac.len(), 1);
    assert_eq!(ac[0]["agent"], "foo");
    assert_eq!(ac[0]["count"], 2);
    assert_eq!(ac[0]["last_role"], "agent");
}

/// Silencia warning de `Map` não utilizado acima (mantido p/ extensão futura).
#[allow(dead_code)]
fn _unused_map() -> Map<String, Value> {
    Map::new()
}
