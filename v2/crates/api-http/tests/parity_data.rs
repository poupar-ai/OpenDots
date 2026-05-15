//! Parity tests — `GET /api/data` Rust vs `plugin/ui/server.cjs::getData()`.
//!
//! Sobe os dois servers em pastas tempfile idênticas, popula `mem_dir` com
//! os mesmos arquivos, e compara o JSON resultante. `project.root` muda
//! entre tempdirs, então é normalizado antes da comparação.
//!
//! Skipa silenciosamente se `node` ou `curl` faltam no PATH (mesmo padrão
//! dos outros `parity_*.rs`).

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

fn mem_dir(tmp: &Path) -> PathBuf {
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

fn curl_get(port: u16, path: &str) -> (u16, Value) {
    let url = format!("http://127.0.0.1:{port}{path}");
    let out = Command::new("curl")
        .args(["-s", "-o", "-", "-w", "\n%{http_code}", &url])
        .output()
        .expect("curl");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let (body, code) = stdout
        .rsplit_once('\n')
        .map(|(b, c)| (b.to_string(), c.to_string()))
        .unwrap_or_else(|| (stdout.clone(), "0".into()));
    let status: u16 = code.trim().parse().unwrap_or(0);
    let body: Value = serde_json::from_str(body.trim()).unwrap_or(Value::Null);
    (status, body)
}

/// Setup: cria duas pastas espelhadas (Rust e Node), popula via `seed`,
/// sobe os 2 servers e devolve. Retorna `None` se faltar node/curl.
fn both<F: Fn(&Path)>(seed: F) -> Option<(TempDir, TempDir, Server, Server)> {
    if !node_available() || !curl_available() {
        eprintln!("skipping — node or curl unavailable");
        return None;
    }
    let tmp_r = TempDir::new().unwrap();
    let tmp_n = TempDir::new().unwrap();
    let mem_r = mem_dir(tmp_r.path());
    let mem_n = mem_dir(tmp_n.path());
    seed(&mem_r);
    seed(&mem_n);
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

/// Normaliza paths que mudam entre tempdirs (project.root, project.name).
/// Deixa o restante intacto pra comparar shape + dados.
fn normalize(mut v: Value) -> Value {
    if let Some(p) = v.get_mut("project").and_then(|p| p.as_object_mut()) {
        p.insert("name".into(), Value::String("<tmp>".into()));
        p.insert("root".into(), Value::String("<tmp>".into()));
    }
    v
}

fn append_jsonl(path: &Path, items: &[Value]) {
    let mut s = String::new();
    for it in items {
        s.push_str(&serde_json::to_string(it).unwrap());
        s.push('\n');
    }
    std::fs::write(path, s).unwrap();
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[test]
fn empty_mem_dir_matches_legacy() {
    let Some((_tr, _tn, sr, sn)) = both(|_| {}) else {
        return;
    };
    let (rs, rb) = curl_get(sr.port, "/api/data");
    let (ns, nb) = curl_get(sn.port, "/api/data");
    assert_eq!(rs, 200);
    assert_eq!(ns, 200);
    assert_eq!(normalize(rb), normalize(nb));
}

#[test]
fn jsonl_arrays_match_legacy() {
    let seed = |mem: &Path| {
        // sessions: 3 entries em 2 ids, com 1 merge e 1 sem ended_at
        // (deve ser filtrada).
        append_jsonl(
            &mem.join("sessions.jsonl"),
            &[
                json!({"session_id": "a", "started_at": "2026-01-01"}),
                json!({"session_id": "a", "ended_at": "2026-01-02"}),
                json!({"session_id": "b", "started_at": "2026-01-03"}),
            ],
        );
        append_jsonl(
            &mem.join("observations.jsonl"),
            &[json!({"a": 1}), json!({"b": 2})],
        );
        append_jsonl(
            &mem.join("decisions.jsonl"),
            &[json!({"d": 1})],
        );
        append_jsonl(
            &mem.join("corrections.jsonl"),
            &[json!({"c": 1}), json!({"c": 2}), json!({"c": 3})],
        );
        append_jsonl(
            &mem.join("gotchas.jsonl"),
            &[json!({"g": 1})],
        );
    };
    let Some((_tr, _tn, sr, sn)) = both(seed) else {
        return;
    };
    let (_, rb) = curl_get(sr.port, "/api/data");
    let (_, nb) = curl_get(sn.port, "/api/data");
    let rb = normalize(rb);
    let nb = normalize(nb);

    // Comparação total (igualdade do shape + dados).
    assert_eq!(rb, nb);

    // Espera: sessions só tem `a` (b sem ended_at é filtrado), reverse
    // dos arrays bate.
    let sessions = rb.get("sessions").and_then(|v| v.as_array()).unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].get("session_id").unwrap(), &json!("a"));
}

#[test]
fn md_files_match_legacy() {
    let seed = |mem: &Path| {
        std::fs::write(mem.join("SOUL.md"), "## Soul\n").unwrap();
        std::fs::write(mem.join("PROJECT.md"), "# Project\n").unwrap();
        std::fs::write(mem.join("DECISIONS.md"), "## Decisions\n").unwrap();
        std::fs::write(mem.join("GOTCHAS.md"), "## Gotchas\n").unwrap();
        std::fs::write(mem.join("GLOSSARY.md"), "## Glossary\n").unwrap();
        std::fs::write(mem.join("CODE_INDEX.md"), "## Code\n").unwrap();
    };
    let Some((_tr, _tn, sr, sn)) = both(seed) else {
        return;
    };
    let (_, rb) = curl_get(sr.port, "/api/data");
    let (_, nb) = curl_get(sn.port, "/api/data");
    assert_eq!(normalize(rb), normalize(nb));
}

#[test]
fn agent_log_summary_match_legacy() {
    let seed = |mem: &Path| {
        // manifest mínimo pra o ramo `agents` retornar não-null.
        std::fs::write(
            mem.join("agents.json"),
            r#"{"agents":{"foo":{"context_file":"foo.md"}}}"#,
        )
        .unwrap();
        let logs_dir = mem.join("agent_logs");
        std::fs::create_dir_all(&logs_dir).unwrap();
        // log com mix de request/response/error
        append_jsonl(
            &logs_dir.join("foo.jsonl"),
            &[
                json!({"type": "request",  "ts": "2026-01-01T00:00:00Z"}),
                json!({"type": "response", "ts": "2026-01-01T00:00:01Z", "duration_ms": 1200}),
                json!({"type": "error",    "ts": "2026-01-01T00:00:02Z",
                       "stderr": "Permission denied", "duration_ms": 50}),
            ],
        );
    };
    let Some((_tr, _tn, sr, sn)) = both(seed) else {
        return;
    };
    let (_, rb) = curl_get(sr.port, "/api/data");
    let (_, nb) = curl_get(sn.port, "/api/data");
    let rb = normalize(rb);
    let nb = normalize(nb);

    // Comparação total — inclui `agents.logs.foo` com counts + hint
    // classificado.
    assert_eq!(rb, nb);

    // Sanity-check pra o caso onde a comparação Rust↔Node passar mas o
    // shape estar errado em AMBOS: garante que o teste de fato exerceu
    // o caminho do `agent_log_summary`.
    let foo = rb
        .pointer("/agents/logs/foo")
        .expect("agents.logs.foo presente");
    assert_eq!(foo.get("total").unwrap(), &json!(3));
    assert_eq!(foo.get("requests").unwrap(), &json!(1));
    assert_eq!(foo.get("responses").unwrap(), &json!(1));
    assert_eq!(foo.get("errors").unwrap(), &json!(1));
    assert_eq!(foo.get("lastEventType").unwrap(), &json!("error"));
    assert!(foo
        .get("lastErrorHint")
        .and_then(|v| v.as_str())
        .map(|s| s.to_lowercase().contains("permission") || s.contains(".claude/settings"))
        .unwrap_or(false));
}

#[test]
fn code_index_summary_match_legacy() {
    let seed = |mem: &Path| {
        std::fs::write(
            mem.join("code_index.json"),
            r#"{
                "generated_at": "2026-01-01T00:00:00Z",
                "files": {
                    "src/a.rs": {"symbols": [{"name":"foo"},{"name":"bar"}]},
                    "src/b.rs": {"symbols": [{"name":"baz"}]}
                }
            }"#,
        )
        .unwrap();
    };
    let Some((_tr, _tn, sr, sn)) = both(seed) else {
        return;
    };
    let (_, rb) = curl_get(sr.port, "/api/data");
    let (_, nb) = curl_get(sn.port, "/api/data");
    assert_eq!(normalize(rb.clone()), normalize(nb));
    let ci = rb.pointer("/stats/codeIndex").unwrap();
    assert_eq!(ci.get("files").unwrap(), &json!(2));
    assert_eq!(ci.get("symbols").unwrap(), &json!(3));
}

#[test]
fn stats_tokens_match_legacy() {
    let seed = |mem: &Path| {
        std::fs::write(
            mem.join("stats.json"),
            r#"{
                "last_injection_tokens": 1234,
                "last_injection_budget": 8000,
                "last_injection_blocks": ["soul","project"],
                "last_injection_skipped": [],
                "last_injection_at": "2026-01-01T00:00:00Z"
            }"#,
        )
        .unwrap();
    };
    let Some((_tr, _tn, sr, sn)) = both(seed) else {
        return;
    };
    let (_, rb) = curl_get(sr.port, "/api/data");
    let (_, nb) = curl_get(sn.port, "/api/data");
    assert_eq!(normalize(rb.clone()), normalize(nb));
    let tk = rb.pointer("/stats/tokens").unwrap();
    assert_eq!(tk.get("last").unwrap(), &json!(1234));
    assert_eq!(tk.get("budget").unwrap(), &json!(8000));
}
