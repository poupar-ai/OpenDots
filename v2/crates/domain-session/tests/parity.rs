//! Cross-language parity tests for `domain-session`.
//!
//! Strategy: extract the exact function bodies from
//! `plugin/scripts/session-init.cjs` and `session-end.cjs` via line
//! ranges, feed them to `node -e` with the closure variables stubbed
//! in, and compare byte-identical output against the Rust port.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use domain_session::{
    classify_session, compact_obs, detect_gotchas, format_observation, is_correction_prompt,
};
use serde_json::{json, Value};

fn scripts_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("plugin/scripts")
}

/// Extracts a contiguous range of lines (1-indexed, inclusive) from a
/// file's source.
fn extract_lines(path: &PathBuf, start: usize, end: usize) -> String {
    let src = fs::read_to_string(path).unwrap();
    src.lines()
        .skip(start - 1)
        .take(end - start + 1)
        .collect::<Vec<_>>()
        .join("\n")
}

fn node_eval(script: &str) -> String {
    let out = Command::new("node")
        .arg("-e")
        .arg(script)
        .output()
        .expect("node invocation failed");
    if !out.status.success() {
        panic!(
            "node failed: stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    String::from_utf8(out.stdout).unwrap()
}

// ─── Helpers bundled from session-init.cjs ──────────────────────────

fn init_helpers_js() -> String {
    let path = scripts_dir().join("session-init.cjs");
    let correction = extract_lines(&path, 46, 46);
    let compact = extract_lines(&path, 64, 68);
    let format = extract_lines(&path, 324, 345);
    format!("'use strict';\n{}\n{}\n{}\n", correction, compact, format)
}

fn end_helpers_js() -> String {
    let path = scripts_dir().join("session-end.cjs");
    let classify = extract_lines(&path, 102, 129);
    let detect = extract_lines(&path, 131, 157);
    // Stub closure vars + appendJsonl + require for graph.cjs.
    format!(
        r#"'use strict';
let sessionId = 'PARITY_SID';
let gotchasFile = '/dev/null';
let memDir = '/tmp/nope';
const __captured = [];
function appendJsonl(_file, obj) {{ __captured.push(obj); }}
const __realRequire = require;
require = function(m) {{
  if (m === './graph.cjs') return {{ onGotcha: () => {{}} }};
  return __realRequire(m);
}};
{classify}
{detect}
"#,
        classify = classify,
        detect = detect,
    )
}

// ─── Parity: compactObs ──────────────────────────────────────────────

fn js_compact(obs: &Value) -> Value {
    let script = format!(
        "{}process.stdout.write(JSON.stringify(compactObs({})));",
        init_helpers_js(),
        obs
    );
    serde_json::from_str(&node_eval(&script)).unwrap()
}

#[test]
fn parity_compact_file_edit() {
    let o = json!({"type": "file_edit", "file": "src/a.rs", "extra": 1});
    assert_eq!(js_compact(&o), compact_obs(&o));
}

#[test]
fn parity_compact_bash_truncates_120() {
    let cmd = "a".repeat(200);
    let o = json!({"type": "bash", "cmd": cmd});
    assert_eq!(js_compact(&o), compact_obs(&o));
}

#[test]
fn parity_compact_bash_missing_cmd() {
    let o = json!({"type": "bash"});
    assert_eq!(js_compact(&o), compact_obs(&o));
}

#[test]
fn parity_compact_other_type() {
    let o = json!({"type": "note", "content": "x"});
    assert_eq!(js_compact(&o), compact_obs(&o));
}

// ─── Parity: formatObs ──────────────────────────────────────────────

fn js_format(ts: &str, obs: &Value) -> String {
    let script = format!(
        "{}process.stdout.write(formatObs({}, {}));",
        init_helpers_js(),
        json!(ts),
        obs
    );
    node_eval(&script)
}

#[test]
fn parity_fmt_file_edit_with_diff() {
    let o = json!({"type": "file_edit", "file": "a.rs", "diff": {"ins": 10, "del": 2}});
    assert_eq!(
        js_format("2026-04-19 10:00", &o),
        format_observation("2026-04-19 10:00", &o)
    );
}

#[test]
fn parity_fmt_file_edit_no_diff() {
    let o = json!({"type": "file_edit", "file": "a.rs"});
    assert_eq!(js_format("t", &o), format_observation("t", &o));
}

#[test]
fn parity_fmt_file_edit_novo() {
    let o = json!({"type": "file_edit", "file": "new.rs", "diff": {"ins": 5, "tracked": false}});
    assert_eq!(js_format("t", &o), format_observation("t", &o));
}

#[test]
fn parity_fmt_bash_exit_zero_summary() {
    let o = json!({"type": "bash", "cmd": "cargo test", "exit": 0, "output_summary": "ok"});
    assert_eq!(js_format("t", &o), format_observation("t", &o));
}

#[test]
fn parity_fmt_bash_exit_missing() {
    let o = json!({"type": "bash", "cmd": "ls"});
    assert_eq!(js_format("t", &o), format_observation("t", &o));
}

#[test]
fn parity_fmt_bash_exit_null() {
    let o = json!({"type": "bash", "cmd": "ls", "exit": null});
    assert_eq!(js_format("t", &o), format_observation("t", &o));
}

#[test]
fn parity_fmt_agent_call() {
    let o = json!({"type": "agent_call", "agent": "manager", "description": "review plan"});
    assert_eq!(js_format("t", &o), format_observation("t", &o));
}

#[test]
fn parity_fmt_note() {
    let o = json!({"type": "note", "content": "rebasing"});
    assert_eq!(js_format("t", &o), format_observation("t", &o));
}

#[test]
fn parity_fmt_unknown_type() {
    let o = json!({"type": "weird"});
    assert_eq!(js_format("t", &o), format_observation("t", &o));
}

// ─── Parity: correction marker regex ─────────────────────────────────

fn js_is_correction(prompt: &str) -> bool {
    // Replicate the full .cjs predicate: short non-empty prompt
    // matching CORRECTION_MARKERS.
    let script = format!(
        r#"{}
const p = {};
if (!p || p.length > 600) process.stdout.write('false');
else process.stdout.write(CORRECTION_MARKERS.test(p) ? 'true' : 'false');
"#,
        init_helpers_js(),
        json!(prompt)
    );
    node_eval(&script) == "true"
}

#[test]
fn parity_correction_marker_cases() {
    let cases = [
        "",
        "não faz isso",
        "nao",
        "pare",
        "stop",
        "don't do that",
        "dont do that",
        "No, that's wrong",
        "undo",
        "revert",
        "   volta aqui",
        "incorreto resultado",
        "incorreta",
        "hello world",
        "yes please",
        "errado",
    ];
    for c in cases {
        let js = js_is_correction(c);
        let rs = is_correction_prompt(c);
        assert_eq!(js, rs, "mismatch on prompt {:?}", c);
    }
}

#[test]
fn parity_correction_too_long() {
    let long = "não ".repeat(300); // 1200 chars
    assert_eq!(js_is_correction(&long), is_correction_prompt(&long));
}

// ─── Parity: classifySession ────────────────────────────────────────

fn js_classify(obs: &[Value]) -> String {
    let arr = Value::Array(obs.to_vec());
    let script = format!(
        "{}process.stdout.write(classifySession({}));",
        end_helpers_js(),
        arr
    );
    node_eval(&script)
}

#[test]
fn parity_classify_docs_readme() {
    let obs = vec![json!({"type": "file_edit", "file": "README.md"})];
    assert_eq!(js_classify(&obs), classify_session(&obs).as_str());
}

#[test]
fn parity_classify_docs_dir() {
    let obs = vec![json!({"type": "file_edit", "file": "docs/setup.md"})];
    assert_eq!(js_classify(&obs), classify_session(&obs).as_str());
}

#[test]
fn parity_classify_config() {
    let obs = vec![
        json!({"type": "file_edit", "file": "docker-compose.yml"}),
        json!({"type": "file_edit", "file": ".env"}),
    ];
    assert_eq!(js_classify(&obs), classify_session(&obs).as_str());
}

#[test]
fn parity_classify_bugfix_by_cmd() {
    let obs = vec![
        json!({"type": "file_edit", "file": "src/a.py"}),
        json!({"type": "bash", "cmd": "git commit -m 'hotfix crash'"}),
    ];
    assert_eq!(js_classify(&obs), classify_session(&obs).as_str());
}

#[test]
fn parity_classify_bugfix_text_and_test() {
    let obs = vec![
        json!({"type": "file_edit", "file": "tests/test_a.py"}),
        json!({"type": "bash", "cmd": "pytest -k fix_bug"}),
    ];
    assert_eq!(js_classify(&obs), classify_session(&obs).as_str());
}

#[test]
fn parity_classify_refactor() {
    let obs = vec![json!({"type": "bash", "cmd": "git mv a b # rename"})];
    assert_eq!(js_classify(&obs), classify_session(&obs).as_str());
}

#[test]
fn parity_classify_feature_src() {
    let obs = vec![json!({"type": "file_edit", "file": "src/api.ts"})];
    assert_eq!(js_classify(&obs), classify_session(&obs).as_str());
}

#[test]
fn parity_classify_feature_src_and_tests() {
    let obs = vec![
        json!({"type": "file_edit", "file": "src/lib.rs"}),
        json!({"type": "file_edit", "file": "tests/test_lib.rs"}),
    ];
    assert_eq!(js_classify(&obs), classify_session(&obs).as_str());
}

#[test]
fn parity_classify_exploration() {
    let obs = vec![json!({"type": "bash", "cmd": "ls"})];
    assert_eq!(js_classify(&obs), classify_session(&obs).as_str());
}

#[test]
fn parity_classify_unknown_empty() {
    let obs: Vec<Value> = vec![];
    assert_eq!(js_classify(&obs), classify_session(&obs).as_str());
}

// ─── Parity: detectGotchas ──────────────────────────────────────────

fn js_detect(obs: &[Value]) -> Value {
    let arr = Value::Array(obs.to_vec());
    let script = format!(
        r#"{}
detectGotchas({});
process.stdout.write(JSON.stringify(__captured));
"#,
        end_helpers_js(),
        arr
    );
    serde_json::from_str(&node_eval(&script)).unwrap()
}

#[test]
fn parity_detect_fail_edit_success() {
    let obs = vec![
        json!({"type": "bash", "cmd": "cargo test foo", "exit": 1, "output_summary": "link err"}),
        json!({"type": "file_edit", "file": "src/lib.rs"}),
        json!({"type": "bash", "cmd": "cargo test foo --release", "exit": 0}),
    ];
    let js = js_detect(&obs);
    let js_arr = js.as_array().unwrap();
    let rs = detect_gotchas(&obs, Some("PARITY_SID"), "IGNORED");
    assert_eq!(js_arr.len(), rs.len());
    assert_eq!(js_arr.len(), 1);
    let j = &js_arr[0];
    let r = &rs[0];
    assert_eq!(j["session_id"], json!("PARITY_SID"));
    assert_eq!(j["file"], r.file);
    assert_eq!(j["command"].as_str().unwrap(), r.command);
    assert_eq!(j["failure_hint"].as_str().unwrap(), "link err");
    assert_eq!(j["description"].as_str().unwrap(), r.description);
}

#[test]
fn parity_detect_prefix_differs_none() {
    let obs = vec![
        json!({"type": "bash", "cmd": "pytest a", "exit": 1}),
        json!({"type": "file_edit", "file": "a.py"}),
        json!({"type": "bash", "cmd": "cargo build", "exit": 0}),
    ];
    let js = js_detect(&obs);
    let rs = detect_gotchas(&obs, Some("PARITY_SID"), "t");
    assert_eq!(js.as_array().unwrap().is_empty(), rs.is_empty());
}

#[test]
fn parity_detect_c_exit_missing_succeeds() {
    // JS uses `c.exit ?? 1`. Missing exit → treated as 1 → blocks.
    // Rust `bash_exit_zero` treats missing as zero — should diverge;
    // but the .cjs semantics: if c.exit is undefined, `?? 1` yields 1,
    // `1 !== 0` is true, so `continue` (no gotcha).
    let obs = vec![
        json!({"type": "bash", "cmd": "pytest", "exit": 1}),
        json!({"type": "file_edit", "file": "a.py"}),
        json!({"type": "bash", "cmd": "pytest"}),
    ];
    let js = js_detect(&obs);
    let rs = detect_gotchas(&obs, Some("PARITY_SID"), "t");
    // This will reveal a divergence — see finding F18-02
    assert_eq!(
        js.as_array().unwrap().len(),
        rs.len(),
        "JS says {} gotchas, Rust says {}",
        js.as_array().unwrap().len(),
        rs.len()
    );
}

#[test]
fn parity_detect_empty() {
    let obs: Vec<Value> = vec![];
    let js = js_detect(&obs);
    let rs = detect_gotchas(&obs, Some("PARITY_SID"), "t");
    assert!(js.as_array().unwrap().is_empty());
    assert!(rs.is_empty());
}

#[test]
fn parity_detect_a_exit_missing() {
    // JS: `(a.exit ?? 0) === 0` — missing exit → 0 → skip (no gotcha).
    let obs = vec![
        json!({"type": "bash", "cmd": "pytest"}),
        json!({"type": "file_edit", "file": "a.py"}),
        json!({"type": "bash", "cmd": "pytest", "exit": 0}),
    ];
    let js = js_detect(&obs);
    let rs = detect_gotchas(&obs, Some("PARITY_SID"), "t");
    assert_eq!(js.as_array().unwrap().len(), rs.len());
}
