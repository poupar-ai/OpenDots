//! Paridade v1 (`agents.cjs::sanitizeTools`/`sanitizeScope`/`updateScope`)
//! vs v2 (`domain_agents::sanitize_tools`/`sanitize_scope`/`update_scope`).
//!
//! Estratégia: despachar operações pelo node recebendo JSON do stdin,
//! comparar saída canonicalizada via `JSON.stringify` / `serde_json`.
//!
//! Cobre os edge cases documentados no porte:
//! - `sanitizeTools` retorna `undefined` pra não-array (sentinela).
//! - `sanitizeScope` coalesce qualquer não-objeto pra `{bash_allow:[]}`.
//! - `updateScope` lança erro se manifest/agente faltam.

use domain_agents::{sanitize_scope, sanitize_tools, update_scope};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::TempDir;

const NODE_SCRIPT: &str = r#"
const lib = require(process.argv[1]);
const op = process.argv[2];
const memDir = process.argv[3] || '';

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', d => { input += d; });
process.stdin.on('end', () => {
  const payload = input ? JSON.parse(input) : null;
  let out;
  try {
    if (op === 'sanitizeTools') {
      const r = lib.sanitizeTools(payload);
      // undefined -> output "undefined" sentinel; arrays serializam normalmente
      out = r === undefined ? 'UNDEFINED' : JSON.stringify(r);
    } else if (op === 'sanitizeScope') {
      out = JSON.stringify(lib.sanitizeScope(payload));
    } else if (op === 'updateScope') {
      const { name, tools, scope } = payload;
      const r = lib.updateScope(memDir, name, { tools, scope });
      out = JSON.stringify(r);
    } else {
      process.stderr.write('op desconhecida: ' + op);
      process.exit(2);
    }
    process.stdout.write(out);
  } catch (e) {
    process.stderr.write('THROW: ' + e.message);
    process.exit(3);
  }
});
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

fn call_v1(op: &str, mem_dir: &Path, payload: &Value) -> Result<String, String> {
    let mut child = Command::new("node")
        .arg("-e")
        .arg(NODE_SCRIPT)
        .arg(lib_cjs_path())
        .arg(op)
        .arg(mem_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn node");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().expect("wait");
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    Ok(String::from_utf8(out.stdout).expect("utf8"))
}

// ── sanitize_tools ─────────────────────────────────────────────────────────

fn assert_tools_match(input: Value) {
    if !node_available() {
        return;
    }
    let dummy = TempDir::new().unwrap();
    let v1 = call_v1("sanitizeTools", dummy.path(), &input).expect("v1 ok");
    let v2 = sanitize_tools(&input);
    let v2_str = match v2 {
        None => "UNDEFINED".to_string(),
        Some(v) => serde_json::to_string(&v).unwrap(),
    };
    assert_eq!(v1, v2_str, "input={input}");
}

#[test]
fn sanitize_tools_non_array() {
    assert_tools_match(json!("x"));
    assert_tools_match(json!(42));
    assert_tools_match(json!(null));
    assert_tools_match(json!({}));
    assert_tools_match(json!(true));
}

#[test]
fn sanitize_tools_empty_array() {
    assert_tools_match(json!([]));
}

#[test]
fn sanitize_tools_all_known() {
    assert_tools_match(json!([
        "Read", "Write", "Edit", "Bash", "Grep", "Glob",
        "WebFetch", "WebSearch", "Task", "TodoWrite", "NotebookEdit"
    ]));
}

#[test]
fn sanitize_tools_filters_unknown() {
    assert_tools_match(json!(["Read", "Foo", "Write", "Bar"]));
}

#[test]
fn sanitize_tools_dedupes_preserving_order() {
    assert_tools_match(json!(["Read", "Write", "Read", "Grep", "Write"]));
}

#[test]
fn sanitize_tools_trims_strings() {
    assert_tools_match(json!([" Read ", "Write\t", "\nGrep"]));
}

#[test]
fn sanitize_tools_non_string_elements() {
    assert_tools_match(json!(["Read", 42, null, true, {}, "Write"]));
}

#[test]
fn sanitize_tools_case_sensitive() {
    // "read" lowercase não é conhecido — JS e Rust ambos rejeitam
    assert_tools_match(json!(["read", "READ", "Read"]));
}

#[test]
fn sanitize_tools_empty_and_whitespace_strings() {
    assert_tools_match(json!(["", "   ", "\t", "Read"]));
}

// ── sanitize_scope ─────────────────────────────────────────────────────────

fn assert_scope_match(input: Value) {
    if !node_available() {
        return;
    }
    let dummy = TempDir::new().unwrap();
    let v1 = call_v1("sanitizeScope", dummy.path(), &input).expect("v1 ok");
    let v2 = sanitize_scope(&input);
    assert_eq!(v1, v2.to_string(), "input={input}");
}

#[test]
fn sanitize_scope_non_object() {
    assert_scope_match(json!(null));
    assert_scope_match(json!("x"));
    assert_scope_match(json!(42));
    assert_scope_match(json!(true));
    assert_scope_match(json!(false));
}

#[test]
fn sanitize_scope_empty_object() {
    assert_scope_match(json!({}));
}

#[test]
fn sanitize_scope_bash_allow_non_array() {
    assert_scope_match(json!({"bash_allow": "git *"}));
    assert_scope_match(json!({"bash_allow": null}));
    assert_scope_match(json!({"bash_allow": 42}));
    assert_scope_match(json!({"bash_allow": {}}));
}

#[test]
fn sanitize_scope_filters_non_string() {
    assert_scope_match(json!({"bash_allow": ["git *", 42, null, true, "npm *"]}));
}

#[test]
fn sanitize_scope_filters_empty_and_whitespace() {
    assert_scope_match(json!({"bash_allow": ["git *", "", "   ", "\t", "npm *"]}));
}

#[test]
fn sanitize_scope_trims() {
    assert_scope_match(json!({"bash_allow": [" git * ", "\tnpm *\n", "ls"]}));
}

#[test]
fn sanitize_scope_caps_at_50() {
    let many: Vec<String> = (0..60).map(|i| format!("cmd{i}")).collect();
    assert_scope_match(json!({"bash_allow": many}));
}

#[test]
fn sanitize_scope_filters_over_200_chars() {
    let long = "a".repeat(201);
    let at_limit = "a".repeat(200);
    assert_scope_match(json!({"bash_allow": [long, at_limit, "git *"]}));
}

#[test]
fn sanitize_scope_ignores_extra_keys() {
    // .cjs só retorna {bash_allow}; campos extras do input são descartados
    assert_scope_match(json!({"bash_allow": ["git *"], "extra": "ignored", "other": [1, 2]}));
}

#[test]
fn sanitize_scope_array_input() {
    // arrays têm typeof 'object' em JS mas `input.bash_allow` é undefined →
    // bashAllow = [] (não early return). Rust faz early return via as_object().
    // Ambos retornam {bash_allow: []}.
    assert_scope_match(json!(["git *"]));
}

// ── update_scope ───────────────────────────────────────────────────────────

fn setup_manifest(dir: &Path, agent: &str) {
    domain_agents::save_manifest(
        dir,
        &json!({
            "version": 1,
            "agents": { agent: { "type": "core", "tools": ["Read"] } }
        }),
    )
    .unwrap();
}

fn twin_dirs(tmp: &TempDir) -> (PathBuf, PathBuf) {
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    (a, b)
}

fn assert_update_match(setup: impl Fn(&Path), payload: Value) {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    setup(&a);
    setup(&b);

    let v1 = call_v1("updateScope", &a, &payload).expect("v1 ok");

    let name = payload["name"].as_str().unwrap();
    let tools = payload.get("tools");
    let scope = payload.get("scope");
    let v2 = update_scope(&b, name, tools, scope).expect("v2 ok");

    assert_eq!(v1, v2.to_string(), "payload={payload}");

    // agents.json também idêntico byte-a-byte
    let ja = std::fs::read(a.join("agents.json")).unwrap();
    let jb = std::fs::read(b.join("agents.json")).unwrap();
    assert_eq!(ja, jb, "agents.json diverge");
}

#[test]
fn update_scope_updates_tools_only() {
    assert_update_match(
        |d| setup_manifest(d, "m"),
        json!({ "name": "m", "tools": ["Read", "Write", "Grep"] }),
    );
}

#[test]
fn update_scope_updates_scope_only() {
    assert_update_match(
        |d| setup_manifest(d, "m"),
        json!({ "name": "m", "scope": { "bash_allow": ["git *", "npm run *"] } }),
    );
}

#[test]
fn update_scope_updates_both() {
    assert_update_match(
        |d| setup_manifest(d, "m"),
        json!({
            "name": "m",
            "tools": ["Read", "Bash"],
            "scope": { "bash_allow": ["git status"] }
        }),
    );
}

#[test]
fn update_scope_tools_non_array_skips() {
    // JS: sanitizeTools(non-array) = undefined → não toca em tools
    assert_update_match(
        |d| setup_manifest(d, "m"),
        json!({ "name": "m", "tools": "Read" }),
    );
}

#[test]
fn update_scope_scope_non_object_resets() {
    // JS: sanitizeScope(non-object) = {bash_allow: []} → sobrescreve
    assert_update_match(
        |d| setup_manifest(d, "m"),
        json!({ "name": "m", "scope": "invalid" }),
    );
}

#[test]
fn update_scope_empty_tools_array() {
    assert_update_match(
        |d| setup_manifest(d, "m"),
        json!({ "name": "m", "tools": [] }),
    );
}

#[test]
fn update_scope_dedupe_and_filter_in_tools() {
    assert_update_match(
        |d| setup_manifest(d, "m"),
        json!({ "name": "m", "tools": ["Read", "Unknown", "Read", "Write"] }),
    );
}

#[test]
fn update_scope_preserves_other_agent_fields() {
    assert_update_match(
        |d| {
            domain_agents::save_manifest(
                d,
                &json!({
                    "version": 1,
                    "agents": {
                        "m": {
                            "type": "core",
                            "role": "Manager",
                            "autonomy": "medium",
                            "tools": ["Read"]
                        }
                    }
                }),
            )
            .unwrap();
        },
        json!({ "name": "m", "scope": { "bash_allow": ["git *"] } }),
    );
}

#[test]
fn update_scope_errors_when_agent_missing() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    setup_manifest(&a, "other");
    setup_manifest(&b, "other");

    let payload = json!({ "name": "missing", "tools": ["Read"] });

    let v1_err = call_v1("updateScope", &a, &payload).expect_err("v1 deve lançar");
    assert!(
        v1_err.contains("missing") || v1_err.contains("agente"),
        "stderr v1: {v1_err}"
    );

    let v2_err = update_scope(&b, "missing", Some(&json!(["Read"])), None)
        .expect_err("v2 deve lançar");
    assert!(matches!(
        v2_err,
        domain_agents::UpdateScopeError::AgentMissing { .. }
    ));
}

#[test]
fn update_scope_errors_when_manifest_missing() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    // nenhum manifest criado
    let payload = json!({ "name": "m", "tools": ["Read"] });
    let v1_err = call_v1("updateScope", &a, &payload).expect_err("v1 deve lançar");
    assert!(
        v1_err.contains("agente") || v1_err.contains("Cannot"),
        "stderr v1: {v1_err}"
    );
    let v2_err = update_scope(&b, "m", Some(&json!(["Read"])), None)
        .expect_err("v2 deve lançar");
    assert!(matches!(
        v2_err,
        domain_agents::UpdateScopeError::AgentMissing { .. }
    ));
}
