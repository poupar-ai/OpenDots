//! Parity tests — `api-http` agents (F4-05e) vs `plugin/ui/server.cjs`.
//!
//! Cobre as 12 rotas de F4-05e:
//! - `POST   /api/agents`                    — create
//! - `DELETE /api/agents/:name`              — delete (bloqueia manager/oracle, handoff=405)
//! - `PATCH  /api/agents/:name/model`        — model + propagate_seed
//! - `PATCH  /api/agents/:name/autonomy`     — autonomy + propagate_seed
//! - `PUT    /api/agents/:name/scope`        — scope + bash_allow (audit log)
//! - `PUT    /api/agents/:name/vault`        — vault access (audit log)
//! - `PUT    /api/agents/:name/context`      — mini-context .md
//! - `PUT    /api/agents/:name/mcps`         — mcps list
//! - `GET    /api/mcps`                      — global/project/referenced/all
//! - `PUT    /api/docs/:doc`                 — SOUL/PROJECT/… allowlist
//! - `GET    /api/agents/log?agent=X`        — tail agent_logs
//! - `GET    /api/agents/work?agent=X`       — observations filtrado

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

/// Remove campos voláteis (timestamps, caminhos absolutos) antes de comparar.
fn strip_volatile(v: &mut Value) {
    match v {
        Value::Object(m) => {
            m.remove("created_at");
            m.remove("updated_at");
            m.remove("last_activity");
            m.remove("last_seen_at");
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

// ── POST /api/agents ────────────────────────────────────────────────────────

#[test]
fn create_invalid_json_400() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl("POST", &rurl(sr.port, "/api/agents"), Some("not-json"));
    let n = curl("POST", &rurl(sn.port, "/api/agents"), Some("not-json"));
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body, json!({ "error": "invalid json" }));
}

#[test]
fn create_missing_name_400() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl("POST", &rurl(sr.port, "/api/agents"), Some(r#"{}"#));
    let n = curl("POST", &rurl(sn.port, "/api/agents"), Some(r#"{}"#));
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body, json!({ "error": "name é obrigatório" }));
}

#[test]
fn create_invalid_type_400() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"name":"foo","type":"bogus"}"#;
    let r = curl("POST", &rurl(sr.port, "/api/agents"), Some(body));
    let n = curl("POST", &rurl(sn.port, "/api/agents"), Some(body));
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
}

#[test]
fn create_specialist_ok() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"name":"alpha","type":"specialist","role":"does stuff"}"#;
    let r = curl("POST", &rurl(sr.port, "/api/agents"), Some(body));
    let n = curl("POST", &rurl(sn.port, "/api/agents"), Some(body));
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    assert_eq!(r.body["ok"], true);
    assert_eq!(n.body["ok"], true);
    assert_eq!(r.body["agent"], "alpha");
    assert_eq!(n.body["agent"], "alpha");
    assert_eq!(r.body["created"]["type"], "specialist");
    assert_eq!(n.body["created"]["type"], "specialist");
}

// ── DELETE /api/agents/:name ────────────────────────────────────────────────

#[test]
fn delete_unknown_404() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl("DELETE", &rurl(sr.port, "/api/agents/ghost"), None);
    let n = curl("DELETE", &rurl(sn.port, "/api/agents/ghost"), None);
    assert_eq!(r.status, 404);
    assert_eq!(n.status, 404);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body, json!({ "error": r#"agent "ghost" not found"# }));
}

#[test]
fn delete_handoff_405() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl("DELETE", &rurl(sr.port, "/api/agents/handoff"), None);
    let n = curl("DELETE", &rurl(sn.port, "/api/agents/handoff"), None);
    assert_eq!(r.status, 405);
    assert_eq!(n.status, 405);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body, json!({ "error": "method not allowed" }));
}

#[test]
fn delete_created_ok() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"name":"bravo","type":"specialist"}"#;
    let _ = curl("POST", &rurl(sr.port, "/api/agents"), Some(body));
    let _ = curl("POST", &rurl(sn.port, "/api/agents"), Some(body));
    let r = curl("DELETE", &rurl(sr.port, "/api/agents/bravo"), None);
    let n = curl("DELETE", &rurl(sn.port, "/api/agents/bravo"), None);
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body, json!({ "ok": true, "agent": "bravo" }));
}

// ── PATCH /api/agents/:name/model ───────────────────────────────────────────

#[test]
fn model_unknown_404() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"model":"sonnet"}"#;
    let r = curl("PATCH", &rurl(sr.port, "/api/agents/ghost/model"), Some(body));
    let n = curl("PATCH", &rurl(sn.port, "/api/agents/ghost/model"), Some(body));
    assert_eq!(r.status, 404);
    assert_eq!(n.status, 404);
    assert_eq!(r.body, n.body);
}

#[test]
fn model_invalid_json_400() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let _ = curl(
        "POST",
        &rurl(sr.port, "/api/agents"),
        Some(r#"{"name":"charlie","type":"specialist"}"#),
    );
    let _ = curl(
        "POST",
        &rurl(sn.port, "/api/agents"),
        Some(r#"{"name":"charlie","type":"specialist"}"#),
    );
    let r = curl("PATCH", &rurl(sr.port, "/api/agents/charlie/model"), Some("xx"));
    let n = curl("PATCH", &rurl(sn.port, "/api/agents/charlie/model"), Some("xx"));
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
}

#[test]
fn model_update_ok() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let _ = curl(
        "POST",
        &rurl(sr.port, "/api/agents"),
        Some(r#"{"name":"delta","type":"specialist"}"#),
    );
    let _ = curl(
        "POST",
        &rurl(sn.port, "/api/agents"),
        Some(r#"{"name":"delta","type":"specialist"}"#),
    );
    let body = r#"{"model":"sonnet"}"#;
    let mut r = curl("PATCH", &rurl(sr.port, "/api/agents/delta/model"), Some(body));
    let mut n = curl("PATCH", &rurl(sn.port, "/api/agents/delta/model"), Some(body));
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    strip_volatile(&mut r.body);
    strip_volatile(&mut n.body);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body["ok"], true);
    assert_eq!(r.body["agent"], "delta");
    assert_eq!(r.body["propagated"], Value::Null);
}

#[test]
fn model_propagate_missing_origin_400() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let _ = curl(
        "POST",
        &rurl(sr.port, "/api/agents"),
        Some(r#"{"name":"echo","type":"specialist"}"#),
    );
    let _ = curl(
        "POST",
        &rurl(sn.port, "/api/agents"),
        Some(r#"{"name":"echo","type":"specialist"}"#),
    );
    let body = r#"{"model":"sonnet","propagate_seed":true}"#;
    let mut r = curl("PATCH", &rurl(sr.port, "/api/agents/echo/model"), Some(body));
    let mut n = curl("PATCH", &rurl(sn.port, "/api/agents/echo/model"), Some(body));
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    strip_volatile(&mut r.body);
    strip_volatile(&mut n.body);
    assert_eq!(r.body, n.body);
    assert!(r.body["error"].as_str().unwrap().contains("propagate_seed"));
}

// ── PATCH /api/agents/:name/autonomy ────────────────────────────────────────

#[test]
fn autonomy_unknown_404() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"level":"manual"}"#;
    let r = curl("PATCH", &rurl(sr.port, "/api/agents/ghost/autonomy"), Some(body));
    let n = curl("PATCH", &rurl(sn.port, "/api/agents/ghost/autonomy"), Some(body));
    assert_eq!(r.status, 404);
    assert_eq!(n.status, 404);
    assert_eq!(r.body, n.body);
}

#[test]
fn autonomy_invalid_level_400() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let _ = curl(
        "POST",
        &rurl(sr.port, "/api/agents"),
        Some(r#"{"name":"foxtrot","type":"specialist"}"#),
    );
    let _ = curl(
        "POST",
        &rurl(sn.port, "/api/agents"),
        Some(r#"{"name":"foxtrot","type":"specialist"}"#),
    );
    let body = r#"{"level":"bogus"}"#;
    let r = curl("PATCH", &rurl(sr.port, "/api/agents/foxtrot/autonomy"), Some(body));
    let n = curl("PATCH", &rurl(sn.port, "/api/agents/foxtrot/autonomy"), Some(body));
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
}

// ── PUT /api/agents/:name/scope ─────────────────────────────────────────────

#[test]
fn scope_unknown_404() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"tools":["Read"]}"#;
    let r = curl("PUT", &rurl(sr.port, "/api/agents/ghost/scope"), Some(body));
    let n = curl("PUT", &rurl(sn.port, "/api/agents/ghost/scope"), Some(body));
    assert_eq!(r.status, 404);
    assert_eq!(n.status, 404);
    assert_eq!(r.body, n.body);
}

#[test]
fn scope_update_ok() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let _ = curl(
        "POST",
        &rurl(sr.port, "/api/agents"),
        Some(r#"{"name":"golf","type":"specialist"}"#),
    );
    let _ = curl(
        "POST",
        &rurl(sn.port, "/api/agents"),
        Some(r#"{"name":"golf","type":"specialist"}"#),
    );
    let body = r#"{"tools":["Read","Write"],"scope":{"bash_allow":["ls"]}}"#;
    let mut r = curl("PUT", &rurl(sr.port, "/api/agents/golf/scope"), Some(body));
    let mut n = curl("PUT", &rurl(sn.port, "/api/agents/golf/scope"), Some(body));
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    strip_volatile(&mut r.body);
    strip_volatile(&mut n.body);
    assert_eq!(r.body["ok"], true);
    assert_eq!(n.body["ok"], true);
    assert_eq!(r.body["agent"], "golf");
    assert_eq!(n.body["agent"], "golf");
}

// ── PUT /api/agents/:name/vault ─────────────────────────────────────────────

#[test]
fn vault_missing_secrets_400() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl("PUT", &rurl(sr.port, "/api/agents/ghost/vault"), Some(r#"{}"#));
    let n = curl("PUT", &rurl(sn.port, "/api/agents/ghost/vault"), Some(r#"{}"#));
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body, json!({ "error": "need {secrets: string[]}" }));
}

#[test]
fn vault_unknown_agent_404() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"secrets":["X"]}"#;
    let r = curl("PUT", &rurl(sr.port, "/api/agents/ghost/vault"), Some(body));
    let n = curl("PUT", &rurl(sn.port, "/api/agents/ghost/vault"), Some(body));
    assert_eq!(r.status, 404);
    assert_eq!(n.status, 404);
    assert_eq!(r.body, n.body);
}

// ── PUT /api/agents/:name/context ───────────────────────────────────────────

#[test]
fn context_missing_content_400() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let _ = curl(
        "POST",
        &rurl(sr.port, "/api/agents"),
        Some(r#"{"name":"hotel","type":"specialist"}"#),
    );
    let _ = curl(
        "POST",
        &rurl(sn.port, "/api/agents"),
        Some(r#"{"name":"hotel","type":"specialist"}"#),
    );
    let r = curl("PUT", &rurl(sr.port, "/api/agents/hotel/context"), Some("{}"));
    let n = curl("PUT", &rurl(sn.port, "/api/agents/hotel/context"), Some("{}"));
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body, json!({ "error": "need {content: string}" }));
}

#[test]
fn context_unknown_agent_404() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"content":"hi"}"#;
    let r = curl("PUT", &rurl(sr.port, "/api/agents/ghost/context"), Some(body));
    let n = curl("PUT", &rurl(sn.port, "/api/agents/ghost/context"), Some(body));
    assert_eq!(r.status, 404);
    assert_eq!(n.status, 404);
    assert_eq!(r.body, n.body);
}

#[test]
fn context_save_ok() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let _ = curl(
        "POST",
        &rurl(sr.port, "/api/agents"),
        Some(r#"{"name":"india","type":"specialist"}"#),
    );
    let _ = curl(
        "POST",
        &rurl(sn.port, "/api/agents"),
        Some(r#"{"name":"india","type":"specialist"}"#),
    );
    let body = r#"{"content":"hello"}"#;
    let r = curl("PUT", &rurl(sr.port, "/api/agents/india/context"), Some(body));
    let n = curl("PUT", &rurl(sn.port, "/api/agents/india/context"), Some(body));
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body["ok"], true);
    assert_eq!(r.body["agent"], "india");
    assert_eq!(r.body["bytes"], 5);
}

// ── PUT /api/agents/:name/mcps ──────────────────────────────────────────────

#[test]
fn mcps_missing_body_400() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl("PUT", &rurl(sr.port, "/api/agents/ghost/mcps"), Some("{}"));
    let n = curl("PUT", &rurl(sn.port, "/api/agents/ghost/mcps"), Some("{}"));
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body, json!({ "error": "need {mcps: string[]}" }));
}

#[test]
fn mcps_unknown_agent_404() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"mcps":["a"]}"#;
    let r = curl("PUT", &rurl(sr.port, "/api/agents/ghost/mcps"), Some(body));
    let n = curl("PUT", &rurl(sn.port, "/api/agents/ghost/mcps"), Some(body));
    assert_eq!(r.status, 404);
    assert_eq!(n.status, 404);
    assert_eq!(r.body, n.body);
}

#[test]
fn mcps_update_ok() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let _ = curl(
        "POST",
        &rurl(sr.port, "/api/agents"),
        Some(r#"{"name":"juliet","type":"specialist"}"#),
    );
    let _ = curl(
        "POST",
        &rurl(sn.port, "/api/agents"),
        Some(r#"{"name":"juliet","type":"specialist"}"#),
    );
    let body = r#"{"mcps":["foo","bar"]}"#;
    let r = curl("PUT", &rurl(sr.port, "/api/agents/juliet/mcps"), Some(body));
    let n = curl("PUT", &rurl(sn.port, "/api/agents/juliet/mcps"), Some(body));
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body, json!({ "ok": true, "agent": "juliet", "mcps": ["foo", "bar"] }));
}

// ── GET /api/mcps ───────────────────────────────────────────────────────────

#[test]
fn mcps_list_shape() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl("GET", &rurl(sr.port, "/api/mcps"), None);
    let n = curl("GET", &rurl(sn.port, "/api/mcps"), None);
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    // `global` e `project` lêem `$HOME/.claude.json` — fora do TempDir, então
    // varia por máquina. Testamos só as chaves + `referenced`/`all` que
    // dependem só do manifest vazio.
    let rk: Vec<&String> = r.body.as_object().map(|m| m.keys().collect()).unwrap_or_default();
    let nk: Vec<&String> = n.body.as_object().map(|m| m.keys().collect()).unwrap_or_default();
    let mut rk: Vec<String> = rk.into_iter().cloned().collect();
    let mut nk: Vec<String> = nk.into_iter().cloned().collect();
    rk.sort();
    nk.sort();
    assert_eq!(rk, nk);
    assert_eq!(r.body["referenced"], json!([]));
    assert_eq!(n.body["referenced"], json!([]));
}

// ── PUT /api/docs/:doc ──────────────────────────────────────────────────────

#[test]
fn doc_save_disallowed_404() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = r#"{"content":"hi"}"#;
    let r = curl("PUT", &rurl(sr.port, "/api/docs/EVIL.md"), Some(body));
    let n = curl("PUT", &rurl(sn.port, "/api/docs/EVIL.md"), Some(body));
    assert_eq!(r.status, 404);
    assert_eq!(n.status, 404);
    assert_eq!(r.body, n.body);
}

#[test]
fn doc_save_missing_content_400() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl("PUT", &rurl(sr.port, "/api/docs/SOUL.md"), Some("{}"));
    let n = curl("PUT", &rurl(sn.port, "/api/docs/SOUL.md"), Some("{}"));
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body, json!({ "error": "need {content: string}" }));
}

#[test]
fn doc_save_ok() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let body = "{\"content\":\"# Soul\"}";
    let r = curl("PUT", &rurl(sr.port, "/api/docs/SOUL.md"), Some(body));
    let n = curl("PUT", &rurl(sn.port, "/api/docs/SOUL.md"), Some(body));
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body, json!({ "ok": true, "doc": "SOUL.md", "bytes": 6 }));
}

// ── GET /api/agents/log ─────────────────────────────────────────────────────

#[test]
fn log_missing_agent_400() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl("GET", &rurl(sr.port, "/api/agents/log"), None);
    let n = curl("GET", &rurl(sn.port, "/api/agents/log"), None);
    assert_eq!(r.status, 400);
    assert_eq!(n.status, 400);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body, json!({ "error": "missing agent" }));
}

#[test]
fn log_empty_returns_empty_entries() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl("GET", &rurl(sr.port, "/api/agents/log?agent=nobody"), None);
    let n = curl("GET", &rurl(sn.port, "/api/agents/log?agent=nobody"), None);
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body, json!({ "agent": "nobody", "entries": [] }));
}

#[test]
fn log_seeded_entries_parity() {
    let Some((tr, tn, sr, sn)) = both_servers() else {
        return;
    };
    let content = r#"{"ts":"2026-04-19T00:00:00.000Z","type":"scope_updated","source":"ui"}
{"ts":"2026-04-19T00:01:00.000Z","type":"vault_access_updated","source":"ui"}
"#;
    for base in [tr.path(), tn.path()] {
        let mem = base.join(".claude-project-memory").join("agent_logs");
        std::fs::create_dir_all(&mem).unwrap();
        std::fs::write(mem.join("kilo.jsonl"), content).unwrap();
    }
    let r = curl("GET", &rurl(sr.port, "/api/agents/log?agent=kilo"), None);
    let n = curl("GET", &rurl(sn.port, "/api/agents/log?agent=kilo"), None);
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    assert_eq!(r.body, n.body);
    let ents = r.body["entries"].as_array().unwrap();
    assert_eq!(ents.len(), 2);
    assert_eq!(ents[0]["type"], "scope_updated");
    assert_eq!(ents[1]["type"], "vault_access_updated");
}

// ── GET /api/agents/work ────────────────────────────────────────────────────

#[test]
fn work_empty_no_filter() {
    let Some((_tr, _tn, sr, sn)) = both_servers() else {
        return;
    };
    let r = curl("GET", &rurl(sr.port, "/api/agents/work"), None);
    let n = curl("GET", &rurl(sn.port, "/api/agents/work"), None);
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body, json!({ "agent": Value::Null, "count": 0, "entries": [] }));
}

#[test]
fn work_seeded_filtered() {
    let Some((tr, tn, sr, sn)) = both_servers() else {
        return;
    };
    let content = r#"{"agent_id":"lima","kind":"edit","ts":"2026-04-19T00:00:00.000Z"}
{"agent_id":"mike","kind":"bash","ts":"2026-04-19T00:01:00.000Z"}
{"agent_id":"lima","kind":"read","ts":"2026-04-19T00:02:00.000Z"}
"#;
    for base in [tr.path(), tn.path()] {
        let mem = base.join(".claude-project-memory");
        std::fs::write(mem.join("observations.jsonl"), content).unwrap();
    }
    let r = curl("GET", &rurl(sr.port, "/api/agents/work?agent=lima"), None);
    let n = curl("GET", &rurl(sn.port, "/api/agents/work?agent=lima"), None);
    assert_eq!(r.status, 200);
    assert_eq!(n.status, 200);
    assert_eq!(r.body, n.body);
    assert_eq!(r.body["count"], 2);
    assert_eq!(r.body["agent"], "lima");
    // reverse slice → último primeiro
    let ents = r.body["entries"].as_array().unwrap();
    assert_eq!(ents[0]["kind"], "read");
    assert_eq!(ents[1]["kind"], "edit");
}
