//! Paridade v1 (`agents.cjs::sanitizeSecretNames`/`updateVaultAccess`)
//! vs v2 (`domain_agents::vault::*`).

use domain_agents::{sanitize_secret_names, save_manifest, update_vault_access};
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::TempDir;

const NODE_SCRIPT: &str = r#"
const lib = require(process.argv[1]);
const op = process.argv[2];
const arg = process.argv[3] || '';

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', d => { input += d; });
process.stdin.on('end', () => {
  const payload = input ? JSON.parse(input) : null;
  try {
    let out;
    if (op === 'sanitizeSecretNames') {
      out = JSON.stringify(lib.sanitizeSecretNames(payload));
    } else if (op === 'updateVaultAccess') {
      out = JSON.stringify(lib.updateVaultAccess(arg, payload.name, { secrets: payload.secrets }));
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

fn call_v1(op: &str, arg: &str, payload: &Value) -> Result<String, String> {
    let mut child = Command::new("node")
        .arg("-e")
        .arg(NODE_SCRIPT)
        .arg(lib_cjs_path())
        .arg(op)
        .arg(arg)
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

fn twin_dirs(tmp: &TempDir) -> (PathBuf, PathBuf) {
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();
    (a, b)
}

// ── sanitize_secret_names ──────────────────────────────────────────────────

fn sanitize_match(input: Value) {
    if !node_available() {
        return;
    }
    let v1 = call_v1("sanitizeSecretNames", "", &input).expect("v1 ok");
    let v2 = sanitize_secret_names(&input);
    assert_eq!(v1, serde_json::to_string(&v2).unwrap(), "input={input}");
}

#[test]
fn sanitize_non_array() {
    sanitize_match(json!(null));
    sanitize_match(json!("x"));
    sanitize_match(json!(42));
    sanitize_match(json!({}));
}

#[test]
fn sanitize_empty() {
    sanitize_match(json!([]));
}

#[test]
fn sanitize_valid_names_sorted() {
    sanitize_match(json!(["Z", "A", "M", "B"]));
}

#[test]
fn sanitize_trims_and_dedupes() {
    sanitize_match(json!(["  API_KEY  ", "API_KEY", "API_KEY\t", "DB_URL"]));
}

#[test]
fn sanitize_mixed_valid_invalid() {
    sanitize_match(json!([
        "API_KEY",
        "lowercase",
        "1_DIGIT",
        "HAS-DASH",
        "HAS SPACE",
        "DD_SITE",
        "",
        null,
        42
    ]));
}

#[test]
fn sanitize_caps_at_50() {
    let many: Vec<String> = (0..60).map(|i| format!("K{i:02}")).collect();
    sanitize_match(json!(many));
}

#[test]
fn sanitize_leading_underscore_rejected() {
    sanitize_match(json!(["_LEADING", "API_KEY"]));
}

#[test]
fn sanitize_over_64_chars_rejected() {
    sanitize_match(json!(["A".repeat(65), "A".repeat(64), "API_KEY"]));
}

#[test]
fn sanitize_sort_is_lexicographic() {
    // underscore (0x5F) vem depois das letras (A-Z = 0x41-0x5A)
    sanitize_match(json!(["A_B", "AA", "A", "AB"]));
}

// ── update_vault_access ────────────────────────────────────────────────────

fn setup(dir: &Path, name: &str) {
    save_manifest(
        dir,
        &json!({
            "version": 1,
            "agents": {name: {"type": "integrator", "role": "X"}}
        }),
    )
    .unwrap();
}

fn update_match(payload: Value) {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let name = payload["name"].as_str().unwrap();
    setup(&a, name);
    setup(&b, name);

    let v1 = call_v1("updateVaultAccess", a.to_str().unwrap(), &payload).expect("v1 ok");
    let v2 = update_vault_access(&b, name, &payload["secrets"]).expect("v2 ok");

    assert_eq!(v1, v2.to_string());
    let ja = fs::read(a.join("agents.json")).unwrap();
    let jb = fs::read(b.join("agents.json")).unwrap();
    assert_eq!(ja, jb);
}

#[test]
fn update_vault_sets_secrets() {
    update_match(json!({"name": "m", "secrets": ["API_KEY", "DB_URL"]}));
}

#[test]
fn update_vault_sorts_secrets() {
    update_match(json!({"name": "m", "secrets": ["Z_LAST", "A_FIRST", "M_MID"]}));
}

#[test]
fn update_vault_dedupes_and_filters() {
    update_match(json!({
        "name": "m",
        "secrets": ["API_KEY", "api_key", "API_KEY", "DB_URL", "1BAD"]
    }));
}

#[test]
fn update_vault_empty_removes_field() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let manifest = json!({
        "version": 1,
        "agents": {"m": {"type": "integrator", "secrets": ["PRE_EXISTING"]}}
    });
    save_manifest(&a, &manifest).unwrap();
    save_manifest(&b, &manifest).unwrap();

    let payload = json!({"name": "m", "secrets": []});
    let v1 = call_v1("updateVaultAccess", a.to_str().unwrap(), &payload).expect("v1");
    let v2 = update_vault_access(&b, "m", &json!([])).expect("v2");
    assert_eq!(v1, v2.to_string());
    let ja = fs::read(a.join("agents.json")).unwrap();
    let jb = fs::read(b.join("agents.json")).unwrap();
    assert_eq!(ja, jb);
}

#[test]
fn update_vault_all_invalid_removes_field() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let manifest = json!({
        "version": 1,
        "agents": {"m": {"type": "integrator", "secrets": ["PRE"]}}
    });
    save_manifest(&a, &manifest).unwrap();
    save_manifest(&b, &manifest).unwrap();

    let payload = json!({"name": "m", "secrets": ["lowercase", "1BAD", ""]});
    let v1 = call_v1("updateVaultAccess", a.to_str().unwrap(), &payload).expect("v1");
    let v2 = update_vault_access(
        &b,
        "m",
        &json!(["lowercase", "1BAD", ""]),
    )
    .expect("v2");
    assert_eq!(v1, v2.to_string());
    let ja = fs::read(a.join("agents.json")).unwrap();
    let jb = fs::read(b.join("agents.json")).unwrap();
    assert_eq!(ja, jb);
}

#[test]
fn update_vault_agent_missing_errors() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    save_manifest(&a, &json!({"version":1,"agents":{}})).unwrap();
    save_manifest(&b, &json!({"version":1,"agents":{}})).unwrap();

    let payload = json!({"name": "ghost", "secrets": ["API_KEY"]});
    let v1 = call_v1("updateVaultAccess", a.to_str().unwrap(), &payload).expect_err("v1");
    assert!(v1.contains("não existe"));
    let v2 =
        update_vault_access(&b, "ghost", &json!(["API_KEY"])).unwrap_err();
    assert!(matches!(
        v2,
        domain_agents::UpdateVaultAccessError::AgentMissing { .. }
    ));
}
