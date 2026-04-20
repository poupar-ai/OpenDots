//! Paridade `pmem-mcp` (Rust) ↔ `plugin/mcp/server.cjs`.
//!
//! Estratégia: dois tempdirs (`.claude-project-memory/` seeded
//! identicamente), spawn de `node server.cjs` e do bin Rust com o
//! **mesmo stdin** (sequência JSON-RPC newline-delimited), diff de
//! cada response line após strip de timestamp dinâmico.
//!
//! Campos stripados:
//! - Não há timestamps nos responses de `mem_context` / `mem_search` /
//!   `mem_read_project` — texto é totalmente determinístico quando o
//!   mem_dir é idêntico.
//! - `tool_add_note` escreve `ts` atual em disco; comparamos via read
//!   do `observations.jsonl` com `ts` removido.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};
use tempfile::TempDir;

fn server_cjs() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("locate repo root")
        .join("plugin/mcp/server.cjs")
}

fn rust_bin() -> &'static str {
    env!("CARGO_BIN_EXE_pmem-mcp")
}

fn mem_dir(root: &Path) -> PathBuf {
    let p = root.join(".claude-project-memory");
    fs::create_dir_all(&p).unwrap();
    p
}

fn run_with_script(cmd: &mut Command, script: &str) -> String {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(script.as_bytes())
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    if !out.status.success() {
        panic!(
            "process exit {:?}: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    String::from_utf8(out.stdout).expect("utf8 stdout")
}

fn run_cjs(cwd: &Path, script: &str) -> String {
    let mut cmd = Command::new("node");
    cmd.arg(server_cjs())
        .current_dir(cwd)
        .env("CLAUDE_CWD", cwd);
    run_with_script(&mut cmd, script)
}

fn run_rust(cwd: &Path, script: &str) -> String {
    let mut cmd = Command::new(rust_bin());
    cmd.current_dir(cwd).env("CLAUDE_CWD", cwd);
    run_with_script(&mut cmd, script)
}

fn parse_responses(raw: &str) -> Vec<Value> {
    raw.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<Value>(l).expect("parse response json"))
        .collect()
}

// ── Tests ──────────────────────────────────────────────────────────────

#[test]
fn initialize_handshake_is_identical() {
    let tmp_js = TempDir::new().unwrap();
    let tmp_rs = TempDir::new().unwrap();
    mem_dir(tmp_js.path());
    mem_dir(tmp_rs.path());

    let script = format!(
        "{}\n",
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize"}),
    );

    let js = run_cjs(tmp_js.path(), &script);
    let rs = run_rust(tmp_rs.path(), &script);

    assert_eq!(parse_responses(&js), parse_responses(&rs));
}

#[test]
fn tools_list_returns_same_four_tools() {
    let tmp_js = TempDir::new().unwrap();
    let tmp_rs = TempDir::new().unwrap();
    mem_dir(tmp_js.path());
    mem_dir(tmp_rs.path());

    let script = format!(
        "{}\n",
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    );

    let js = parse_responses(&run_cjs(tmp_js.path(), &script));
    let rs = parse_responses(&run_rust(tmp_rs.path(), &script));
    assert_eq!(js, rs);
    assert_eq!(js[0]["result"]["tools"].as_array().unwrap().len(), 4);
}

#[test]
fn notifications_initialized_produces_no_response() {
    let tmp_js = TempDir::new().unwrap();
    let tmp_rs = TempDir::new().unwrap();
    mem_dir(tmp_js.path());
    mem_dir(tmp_rs.path());

    let script = format!(
        "{}\n",
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    );

    let js = run_cjs(tmp_js.path(), &script);
    let rs = run_rust(tmp_rs.path(), &script);
    assert_eq!(js.trim(), "");
    assert_eq!(rs.trim(), "");
}

#[test]
fn mem_read_project_missing() {
    let tmp_js = TempDir::new().unwrap();
    let tmp_rs = TempDir::new().unwrap();
    mem_dir(tmp_js.path());
    mem_dir(tmp_rs.path());

    let script = format!(
        "{}\n",
        json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": { "name": "mem_read_project" }
        }),
    );

    let js = parse_responses(&run_cjs(tmp_js.path(), &script));
    let rs = parse_responses(&run_rust(tmp_rs.path(), &script));
    assert_eq!(js, rs);
}

#[test]
fn mem_read_project_with_content() {
    let tmp_js = TempDir::new().unwrap();
    let tmp_rs = TempDir::new().unwrap();
    for root in [tmp_js.path(), tmp_rs.path()] {
        let md = mem_dir(root);
        fs::write(md.join("PROJECT.md"), "# Meu projeto\n\nConteúdo.").unwrap();
    }

    let script = format!(
        "{}\n",
        json!({
            "jsonrpc": "2.0", "id": 10, "method": "tools/call",
            "params": { "name": "mem_read_project" }
        }),
    );

    let js = parse_responses(&run_cjs(tmp_js.path(), &script));
    let rs = parse_responses(&run_rust(tmp_rs.path(), &script));
    assert_eq!(js, rs);
}

#[test]
fn mem_context_with_full_state() {
    let tmp_js = TempDir::new().unwrap();
    let tmp_rs = TempDir::new().unwrap();
    let project = "# T\n\nconteúdo do projeto.";
    let sessions =
        "{\"type\":\"end\",\"summary\":\"Fez X\",\"ended_at\":\"2026-04-10T10:00:00.000Z\"}\n\
        {\"type\":\"end\",\"summary\":\"Fez Y\",\"ended_at\":\"2026-04-11T10:00:00.000Z\"}\n";
    let obs =
        "{\"type\":\"file_edit\",\"file\":\"src/a.rs\",\"ts\":\"2026-04-11T09:00:00.000Z\"}\n\
        {\"type\":\"bash\",\"cmd\":\"cargo test\",\"ts\":\"2026-04-11T09:05:00.000Z\"}\n\
        {\"type\":\"note\",\"content\":\"uma nota\",\"ts\":\"2026-04-11T09:10:00.000Z\"}\n";

    for root in [tmp_js.path(), tmp_rs.path()] {
        let md = mem_dir(root);
        fs::write(md.join("PROJECT.md"), project).unwrap();
        fs::write(md.join("sessions.jsonl"), sessions).unwrap();
        fs::write(md.join("observations.jsonl"), obs).unwrap();
    }

    let script = format!(
        "{}\n",
        json!({
            "jsonrpc": "2.0", "id": 4, "method": "tools/call",
            "params": { "name": "mem_context" }
        }),
    );

    let js = parse_responses(&run_cjs(tmp_js.path(), &script));
    let rs = parse_responses(&run_rust(tmp_rs.path(), &script));
    assert_eq!(js, rs);
}

#[test]
fn mem_search_case_insensitive() {
    let tmp_js = TempDir::new().unwrap();
    let tmp_rs = TempDir::new().unwrap();
    let obs = "{\"type\":\"bash\",\"cmd\":\"CARGO TEST\",\"ts\":\"2026-04-11T09:00:00.000Z\"}\n\
        {\"type\":\"file_edit\",\"file\":\"src/cargo_util.rs\",\"ts\":\"2026-04-10T08:00:00.000Z\"}\n";
    for root in [tmp_js.path(), tmp_rs.path()] {
        let md = mem_dir(root);
        fs::write(md.join("observations.jsonl"), obs).unwrap();
    }

    let script = format!(
        "{}\n",
        json!({
            "jsonrpc": "2.0", "id": 5, "method": "tools/call",
            "params": { "name": "mem_search", "arguments": { "query": "cargo" } }
        }),
    );

    let js = parse_responses(&run_cjs(tmp_js.path(), &script));
    let rs = parse_responses(&run_rust(tmp_rs.path(), &script));
    assert_eq!(js, rs);
}

#[test]
fn mem_search_type_file_only() {
    let tmp_js = TempDir::new().unwrap();
    let tmp_rs = TempDir::new().unwrap();
    let obs = "{\"type\":\"bash\",\"cmd\":\"cargo main\",\"ts\":\"2026-04-11T09:00:00.000Z\"}\n\
        {\"type\":\"file_edit\",\"file\":\"src/main.rs\",\"ts\":\"2026-04-10T08:00:00.000Z\"}\n";
    for root in [tmp_js.path(), tmp_rs.path()] {
        let md = mem_dir(root);
        fs::write(md.join("observations.jsonl"), obs).unwrap();
    }

    let script = format!(
        "{}\n",
        json!({
            "jsonrpc": "2.0", "id": 6, "method": "tools/call",
            "params": { "name": "mem_search", "arguments": { "query": "main", "type": "file" } }
        }),
    );

    let js = parse_responses(&run_cjs(tmp_js.path(), &script));
    let rs = parse_responses(&run_rust(tmp_rs.path(), &script));
    assert_eq!(js, rs);
}

#[test]
fn mem_search_no_results_message() {
    let tmp_js = TempDir::new().unwrap();
    let tmp_rs = TempDir::new().unwrap();
    for root in [tmp_js.path(), tmp_rs.path()] {
        mem_dir(root);
    }

    let script = format!(
        "{}\n",
        json!({
            "jsonrpc": "2.0", "id": 7, "method": "tools/call",
            "params": { "name": "mem_search", "arguments": { "query": "qq" } }
        }),
    );

    let js = parse_responses(&run_cjs(tmp_js.path(), &script));
    let rs = parse_responses(&run_rust(tmp_rs.path(), &script));
    assert_eq!(js, rs);
}

#[test]
fn mem_add_note_persists_to_observations_jsonl() {
    let tmp_js = TempDir::new().unwrap();
    let tmp_rs = TempDir::new().unwrap();
    for root in [tmp_js.path(), tmp_rs.path()] {
        mem_dir(root);
    }

    let script = format!(
        "{}\n",
        json!({
            "jsonrpc": "2.0", "id": 8, "method": "tools/call",
            "params": { "name": "mem_add_note", "arguments": { "content": " decisão importante  " } }
        }),
    );

    let js = parse_responses(&run_cjs(tmp_js.path(), &script));
    let rs = parse_responses(&run_rust(tmp_rs.path(), &script));
    assert_eq!(js, rs, "response do add_note difere");

    // Conteúdo persistido: mesma shape, ts stripado por ser dinâmico.
    let strip_ts = |path: &Path| -> Vec<Value> {
        fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                let mut v: Value = serde_json::from_str(l).unwrap();
                if let Some(m) = v.as_object_mut() {
                    m.remove("ts");
                }
                v
            })
            .collect()
    };
    let js_obs = strip_ts(
        &tmp_js
            .path()
            .join(".claude-project-memory/observations.jsonl"),
    );
    let rs_obs = strip_ts(
        &tmp_rs
            .path()
            .join(".claude-project-memory/observations.jsonl"),
    );
    assert_eq!(js_obs, rs_obs, "observations.jsonl difere após add_note");
}

#[test]
fn mem_add_note_empty_returns_error_message() {
    let tmp_js = TempDir::new().unwrap();
    let tmp_rs = TempDir::new().unwrap();
    for root in [tmp_js.path(), tmp_rs.path()] {
        mem_dir(root);
    }

    let script = format!(
        "{}\n",
        json!({
            "jsonrpc": "2.0", "id": 9, "method": "tools/call",
            "params": { "name": "mem_add_note", "arguments": { "content": "   " } }
        }),
    );

    let js = parse_responses(&run_cjs(tmp_js.path(), &script));
    let rs = parse_responses(&run_rust(tmp_rs.path(), &script));
    assert_eq!(js, rs);
}

#[test]
fn unknown_tool_name_returns_error() {
    let tmp_js = TempDir::new().unwrap();
    let tmp_rs = TempDir::new().unwrap();
    for root in [tmp_js.path(), tmp_rs.path()] {
        mem_dir(root);
    }

    let script = format!(
        "{}\n",
        json!({
            "jsonrpc": "2.0", "id": 11, "method": "tools/call",
            "params": { "name": "bogus", "arguments": {} }
        }),
    );

    let js = parse_responses(&run_cjs(tmp_js.path(), &script));
    let rs = parse_responses(&run_rust(tmp_rs.path(), &script));
    assert_eq!(js, rs);
}

#[test]
fn full_session_roundtrip_initialize_list_context_note() {
    let tmp_js = TempDir::new().unwrap();
    let tmp_rs = TempDir::new().unwrap();
    for root in [tmp_js.path(), tmp_rs.path()] {
        let md = mem_dir(root);
        fs::write(md.join("PROJECT.md"), "# proj").unwrap();
    }

    let script = format!(
        "{}\n{}\n{}\n{}\n",
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": { "name": "mem_read_project" }
        }),
        json!({
            "jsonrpc": "2.0", "id": 4, "method": "tools/call",
            "params": { "name": "mem_add_note", "arguments": { "content": "hello" } }
        }),
    );

    let js = parse_responses(&run_cjs(tmp_js.path(), &script));
    let rs = parse_responses(&run_rust(tmp_rs.path(), &script));
    assert_eq!(js.len(), 4);
    assert_eq!(rs.len(), 4);
    assert_eq!(js, rs);
}

#[test]
fn unknown_method_without_id_is_silent() {
    let tmp_js = TempDir::new().unwrap();
    let tmp_rs = TempDir::new().unwrap();
    for root in [tmp_js.path(), tmp_rs.path()] {
        mem_dir(root);
    }

    let script = format!("{}\n", json!({"jsonrpc": "2.0", "method": "foo/bar"}),);

    let js = run_cjs(tmp_js.path(), &script);
    let rs = run_rust(tmp_rs.path(), &script);
    assert_eq!(js.trim(), "");
    assert_eq!(rs.trim(), "");
}

#[test]
fn malformed_json_line_is_ignored() {
    let tmp_js = TempDir::new().unwrap();
    let tmp_rs = TempDir::new().unwrap();
    for root in [tmp_js.path(), tmp_rs.path()] {
        mem_dir(root);
    }

    // Linha inválida seguida de uma válida. Ambos os lados devem
    // descartar a primeira e responder apenas à segunda.
    let script = format!(
        "not-json\n{}\n",
        json!({"jsonrpc": "2.0", "id": 42, "method": "tools/list"}),
    );

    let js = parse_responses(&run_cjs(tmp_js.path(), &script));
    let rs = parse_responses(&run_rust(tmp_rs.path(), &script));
    assert_eq!(js.len(), 1);
    assert_eq!(rs.len(), 1);
    assert_eq!(js, rs);
}
