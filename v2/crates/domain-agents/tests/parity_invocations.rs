//! Paridade v1 (`agents.cjs::countInvocations`) vs v2
//! (`domain_agents::count_invocations`).
//!
//! Estratégia: escrever `observations.jsonl` em dois dirs irmãos,
//! rodar cada lado, comparar serialização JSON estável. Cobre os
//! edge cases documentados no porte (null line, numeric agent,
//! ts comparison, etc).

use domain_agents::count_invocations;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

const NODE_SCRIPT: &str = r#"
const lib = require(process.argv[1]);
const memDir = process.argv[2];
const r = lib.countInvocations(memDir);
process.stdout.write(JSON.stringify(r));
"#;

fn lib_cjs_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../plugin/scripts/agents.cjs")
        .canonicalize()
        .expect("agents.cjs missing")
}

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn call_v1(mem_dir: &Path) -> String {
    let out = Command::new("node")
        .arg("-e")
        .arg(NODE_SCRIPT)
        .arg(lib_cjs_path())
        .arg(mem_dir)
        .output()
        .expect("spawn node");
    assert!(
        out.status.success(),
        "node falhou: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf8")
}

fn twin_dirs(tmp: &TempDir, obs: Option<&str>) -> (PathBuf, PathBuf) {
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();
    if let Some(content) = obs {
        fs::write(a.join("observations.jsonl"), content).unwrap();
        fs::write(b.join("observations.jsonl"), content).unwrap();
    }
    (a, b)
}

fn assert_parity(obs: Option<&str>) {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, obs);
    let v1 = call_v1(&a);
    let v2 = count_invocations(&b);
    let v2_str = serde_json::to_string(&v2).unwrap();
    assert_eq!(v1, v2_str, "obs={:?}", obs);
}

#[test]
fn parity_missing_file_returns_empty_object() {
    assert_parity(None);
}

#[test]
fn parity_empty_file() {
    assert_parity(Some(""));
}

#[test]
fn parity_single_entry() {
    assert_parity(Some(
        r#"{"type":"agent_call","agent":"manager","ts":"2026-04-18T10:00:00Z"}"#,
    ));
}

#[test]
fn parity_multiple_same_agent() {
    assert_parity(Some(
        "{\"type\":\"agent_call\",\"agent\":\"manager\",\"ts\":\"2026-04-18T10:00:00Z\"}\n\
         {\"type\":\"agent_call\",\"agent\":\"manager\",\"ts\":\"2026-04-18T11:00:00Z\"}\n\
         {\"type\":\"agent_call\",\"agent\":\"manager\",\"ts\":\"2026-04-18T12:00:00Z\"}",
    ));
}

#[test]
fn parity_multiple_agents_order_preserved() {
    assert_parity(Some(
        "{\"type\":\"agent_call\",\"agent\":\"manager\",\"ts\":\"2026-04-18T10:00:00Z\"}\n\
         {\"type\":\"agent_call\",\"agent\":\"oracle\",\"ts\":\"2026-04-18T11:00:00Z\"}\n\
         {\"type\":\"agent_call\",\"agent\":\"architect\",\"ts\":\"2026-04-18T12:00:00Z\"}",
    ));
}

#[test]
fn parity_ignores_non_agent_call_type() {
    assert_parity(Some(
        "{\"type\":\"session_start\",\"agent\":\"manager\",\"ts\":\"2026-04-18\"}\n\
         {\"type\":\"agent_call\",\"agent\":\"manager\",\"ts\":\"2026-04-18T11:00:00Z\"}",
    ));
}

#[test]
fn parity_case_insensitive_agent() {
    assert_parity(Some(
        "{\"type\":\"agent_call\",\"agent\":\"Manager\",\"ts\":\"2026-01\"}\n\
         {\"type\":\"agent_call\",\"agent\":\"MANAGER\",\"ts\":\"2026-02\"}\n\
         {\"type\":\"agent_call\",\"agent\":\"manager\",\"ts\":\"2026-03\"}",
    ));
}

#[test]
fn parity_skips_invalid_json() {
    assert_parity(Some(
        "not-json\n\
         {incomplete\n\
         {\"type\":\"agent_call\",\"agent\":\"manager\",\"ts\":\"2026\"}",
    ));
}

#[test]
fn parity_empty_agent_string() {
    assert_parity(Some(
        "{\"type\":\"agent_call\",\"agent\":\"\",\"ts\":\"2026\"}\n\
         {\"type\":\"agent_call\",\"agent\":\"real\",\"ts\":\"2026\"}",
    ));
}

#[test]
fn parity_missing_agent_field() {
    assert_parity(Some(
        "{\"type\":\"agent_call\",\"ts\":\"2026\"}\n\
         {\"type\":\"agent_call\",\"agent\":\"real\",\"ts\":\"2026\"}",
    ));
}

#[test]
fn parity_recent_lexicographic_max() {
    assert_parity(Some(
        "{\"type\":\"agent_call\",\"agent\":\"m\",\"ts\":\"2026-03\"}\n\
         {\"type\":\"agent_call\",\"agent\":\"m\",\"ts\":\"2026-01\"}\n\
         {\"type\":\"agent_call\",\"agent\":\"m\",\"ts\":\"2026-02\"}",
    ));
}

#[test]
fn parity_null_line_breaks_loop() {
    // JS: null.type lança TypeError, swallow pelo outer try/catch,
    // loop para. Entradas anteriores contam; posteriores não.
    assert_parity(Some(
        "{\"type\":\"agent_call\",\"agent\":\"before_null\",\"ts\":\"1\"}\n\
         null\n\
         {\"type\":\"agent_call\",\"agent\":\"after_null\",\"ts\":\"2\"}",
    ));
}

#[test]
fn parity_numeric_agent_breaks_loop() {
    // JS: (42).toLowerCase lança, outer try/catch swallow, loop para.
    assert_parity(Some(
        "{\"type\":\"agent_call\",\"agent\":\"before\",\"ts\":\"1\"}\n\
         {\"type\":\"agent_call\",\"agent\":42,\"ts\":\"2\"}\n\
         {\"type\":\"agent_call\",\"agent\":\"after\",\"ts\":\"3\"}",
    ));
}

#[test]
fn parity_object_agent_breaks_loop() {
    // JS: ({}).toLowerCase → undefined → TypeError → loop para.
    assert_parity(Some(
        "{\"type\":\"agent_call\",\"agent\":\"before\",\"ts\":\"1\"}\n\
         {\"type\":\"agent_call\",\"agent\":{\"x\":1},\"ts\":\"2\"}\n\
         {\"type\":\"agent_call\",\"agent\":\"after\",\"ts\":\"3\"}",
    ));
}

#[test]
fn parity_array_agent_breaks_loop() {
    assert_parity(Some(
        "{\"type\":\"agent_call\",\"agent\":[],\"ts\":\"1\"}\n\
         {\"type\":\"agent_call\",\"agent\":\"after\",\"ts\":\"2\"}",
    ));
}

#[test]
fn parity_primitive_line_continues() {
    // JS: (42).type → undefined → `!== 'agent_call'` → continue. OK.
    assert_parity(Some(
        "42\n\
         \"string\"\n\
         true\n\
         {\"type\":\"agent_call\",\"agent\":\"m\",\"ts\":\"1\"}",
    ));
}

#[test]
fn parity_false_agent_skipped() {
    // `(false || '').toLowerCase()` = '' → skip (continue).
    assert_parity(Some(
        "{\"type\":\"agent_call\",\"agent\":false,\"ts\":\"1\"}\n\
         {\"type\":\"agent_call\",\"agent\":\"m\",\"ts\":\"2\"}",
    ));
}

#[test]
fn parity_null_agent_skipped() {
    // (null || '').toLowerCase() = '' → skip.
    assert_parity(Some(
        "{\"type\":\"agent_call\",\"agent\":null,\"ts\":\"1\"}\n\
         {\"type\":\"agent_call\",\"agent\":\"m\",\"ts\":\"2\"}",
    ));
}

#[test]
fn parity_zero_agent_skipped() {
    // `(0 || '').toLowerCase()` = '' → skip.
    assert_parity(Some(
        "{\"type\":\"agent_call\",\"agent\":0,\"ts\":\"1\"}\n\
         {\"type\":\"agent_call\",\"agent\":\"m\",\"ts\":\"2\"}",
    ));
}

#[test]
fn parity_blank_lines_filtered() {
    assert_parity(Some(
        "\n\n{\"type\":\"agent_call\",\"agent\":\"m\",\"ts\":\"1\"}\n\n\
         {\"type\":\"agent_call\",\"agent\":\"m\",\"ts\":\"2\"}\n\n",
    ));
}

#[test]
fn parity_mixed_types_full_scenario() {
    assert_parity(Some(
        "{\"type\":\"session_start\",\"ts\":\"2026-04-18T08:00:00Z\"}\n\
         {\"type\":\"agent_call\",\"agent\":\"manager\",\"ts\":\"2026-04-18T09:00:00Z\"}\n\
         {\"type\":\"agent_call\",\"agent\":\"Oracle\",\"ts\":\"2026-04-18T10:00:00Z\"}\n\
         not-json-garbage\n\
         {\"type\":\"observation\",\"message\":\"hi\"}\n\
         {\"type\":\"agent_call\",\"agent\":\"oracle\",\"ts\":\"2026-04-18T11:00:00Z\"}\n\
         {\"type\":\"agent_call\",\"agent\":\"architect\",\"ts\":\"2026-04-18T12:00:00Z\"}",
    ));
}
