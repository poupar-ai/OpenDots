//! Paridade v1 (`agents.cjs::normalizeModel`/`resolveCliModel`/
//! `updateModel`/`propagateModelToSeed`/`CLI_MODEL_MAP`) vs v2
//! (`domain_agents::model::*`).

use domain_agents::{
    normalize_model, propagate_model_to_seed, resolve_cli_model, save_manifest, update_model,
    CLI_MODEL_MAP,
};
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
    if (op === 'normalizeModel') {
      const r = lib.normalizeModel(payload);
      out = r === null ? 'NULL' : JSON.stringify(r);
    } else if (op === 'resolveCliModel') {
      const r = lib.resolveCliModel(payload);
      out = r === null ? 'NULL' : JSON.stringify(r);
    } else if (op === 'cliModelMap') {
      out = JSON.stringify(lib.CLI_MODEL_MAP);
    } else if (op === 'updateModel') {
      out = JSON.stringify(lib.updateModel(arg, payload.name, payload.model));
    } else if (op === 'propagateModelToSeed') {
      const r = lib.propagateModelToSeed(arg, payload.name, payload.model);
      out = r === null ? 'NULL' : JSON.stringify(r);
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

// CLI_MODEL_MAP não é exportado do `.cjs` — a paridade é coberta
// indiretamente por `resolve_all_map_keys_parity` que itera cada
// chave do Rust e compara a saída de `resolveCliModel` contra o JS.

// ── normalize_model ────────────────────────────────────────────────────────

fn norm_match(input: Value) {
    if !node_available() {
        return;
    }
    let v1 = call_v1("normalizeModel", "", &input).expect("v1 ok");
    let v2 = normalize_model(&input).expect("v2 ok");
    let v2_str = match v2 {
        None => "NULL".to_string(),
        Some(s) => serde_json::to_string(&s).unwrap(),
    };
    assert_eq!(v1, v2_str, "input={input}");
}

#[test]
fn normalize_model_null() {
    norm_match(json!(null));
}

#[test]
fn normalize_model_empty() {
    norm_match(json!(""));
}

#[test]
fn normalize_model_whitespace() {
    norm_match(json!("   "));
    norm_match(json!("\t\n"));
}

#[test]
fn normalize_model_trims() {
    norm_match(json!("  opus  "));
    norm_match(json!("\topus-4.7\t"));
}

#[test]
fn normalize_model_accepts_80_chars() {
    norm_match(json!("a".repeat(80)));
}

#[test]
fn normalize_model_error_non_string() {
    if !node_available() {
        return;
    }
    for input in [json!(42), json!(true), json!([]), json!({})] {
        let v1 = call_v1("normalizeModel", "", &input).expect_err("v1");
        assert!(v1.contains("precisa ser string"), "v1={v1}");
        assert!(normalize_model(&input).is_err(), "v2 erro pra {input}");
    }
}

#[test]
fn normalize_model_error_too_long() {
    if !node_available() {
        return;
    }
    let input = json!("a".repeat(81));
    let v1 = call_v1("normalizeModel", "", &input).expect_err("v1");
    assert!(v1.contains("muito longo"), "v1={v1}");
    assert!(normalize_model(&input).is_err(), "v2 erro");
}

// ── resolve_cli_model ──────────────────────────────────────────────────────

fn resolve_match(input: Value) {
    if !node_available() {
        return;
    }
    let v1 = call_v1("resolveCliModel", "", &input).expect("v1 ok");
    let v2 = resolve_cli_model(&input);
    let v2_str = match v2 {
        None => "NULL".to_string(),
        Some(s) => serde_json::to_string(&s).unwrap(),
    };
    assert_eq!(v1, v2_str, "input={input}");
}

#[test]
fn resolve_pure_aliases_parity() {
    resolve_match(json!("opus"));
    resolve_match(json!("sonnet"));
    resolve_match(json!("haiku"));
}

#[test]
fn resolve_claude_prefix_parity() {
    resolve_match(json!("claude-opus-4-7"));
    resolve_match(json!("claude-sonnet-4-6"));
    resolve_match(json!("claude-haiku-4-5-20251001"));
    resolve_match(json!("claude-anything-goes"));
}

#[test]
fn resolve_all_map_keys_parity() {
    for (k, _) in CLI_MODEL_MAP {
        resolve_match(json!(*k));
    }
}

#[test]
fn resolve_fallback_pattern_parity() {
    // não está no map, casa `^(opus|sonnet|haiku)-\d+\.\d+$`
    resolve_match(json!("opus-9.9"));
    resolve_match(json!("sonnet-10.0"));
    resolve_match(json!("haiku-2.0"));
}

#[test]
fn resolve_unknown_passthrough_parity() {
    resolve_match(json!("foo-bar"));
    resolve_match(json!("random"));
    resolve_match(json!("opus-bad"));
    resolve_match(json!("opus-1.2.3")); // regex exige exato "N.N"
}

#[test]
fn resolve_null_and_empty() {
    resolve_match(json!(null));
    resolve_match(json!(""));
    resolve_match(json!("   "));
    resolve_match(json!(42));
    resolve_match(json!(true));
}

#[test]
fn resolve_trims() {
    resolve_match(json!("  opus  "));
    resolve_match(json!("  opus-4.7  "));
    resolve_match(json!("\tclaude-opus-4-7\n"));
}

// ── update_model ───────────────────────────────────────────────────────────

fn setup_agent(dir: &Path, name: &str) {
    save_manifest(
        dir,
        &json!({
            "version": 1,
            "agents": {name: {"type": "specialist", "role": "X"}}
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
    setup_agent(&a, name);
    setup_agent(&b, name);

    let v1 = call_v1("updateModel", a.to_str().unwrap(), &payload).expect("v1 ok");
    let v2 = update_model(&b, name, &payload["model"]).expect("v2 ok");

    assert_eq!(v1, v2.to_string());
    let ja = fs::read(a.join("agents.json")).unwrap();
    let jb = fs::read(b.join("agents.json")).unwrap();
    assert_eq!(ja, jb);
}

#[test]
fn update_model_sets() {
    update_match(json!({"name": "m", "model": "opus-4.7"}));
}

#[test]
fn update_model_trims() {
    update_match(json!({"name": "m", "model": "  opus  "}));
}

#[test]
fn update_model_empty_clears() {
    update_match(json!({"name": "m", "model": ""}));
}

#[test]
fn update_model_null_removes_existing() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let manifest = json!({
        "version": 1,
        "agents": {"m": {"type": "specialist", "model": "opus"}}
    });
    save_manifest(&a, &manifest).unwrap();
    save_manifest(&b, &manifest).unwrap();

    let payload = json!({"name": "m", "model": null});
    let v1 = call_v1("updateModel", a.to_str().unwrap(), &payload).expect("v1");
    let v2 = update_model(&b, "m", &json!(null)).expect("v2");
    assert_eq!(v1, v2.to_string());
    let ja = fs::read(a.join("agents.json")).unwrap();
    let jb = fs::read(b.join("agents.json")).unwrap();
    assert_eq!(ja, jb);
}

#[test]
fn update_model_errors_agent_missing() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    save_manifest(&a, &json!({"version":1,"agents":{}})).unwrap();
    save_manifest(&b, &json!({"version":1,"agents":{}})).unwrap();

    let payload = json!({"name": "ghost", "model": "opus"});
    let v1 = call_v1("updateModel", a.to_str().unwrap(), &payload).expect_err("v1");
    assert!(v1.contains("não existe"));
    let v2 = update_model(&b, "ghost", &json!("opus")).unwrap_err();
    assert!(matches!(
        v2,
        domain_agents::UpdateModelError::AgentMissing { .. }
    ));
}

// ── propagate_model_to_seed ────────────────────────────────────────────────

fn propagate_match(initial: Value, payload: Value) {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let pa = tmp.path().join("a.json");
    let pb = tmp.path().join("b.json");
    let init = serde_json::to_string_pretty(&initial).unwrap() + "\n";
    fs::write(&pa, &init).unwrap();
    fs::write(&pb, &init).unwrap();

    let v1 = call_v1("propagateModelToSeed", pa.to_str().unwrap(), &payload).expect("v1 ok");
    let v2 = propagate_model_to_seed(&pb, payload["name"].as_str().unwrap(), &payload["model"])
        .expect("v2 ok");
    let v2_str = match v2 {
        None => "NULL".to_string(),
        Some(v) => serde_json::to_string(&v).unwrap(),
    };
    assert_eq!(v1, v2_str);
    let ba = fs::read(&pa).unwrap();
    let bb = fs::read(&pb).unwrap();
    assert_eq!(ba, bb);
}

#[test]
fn propagate_model_sets() {
    propagate_match(
        json!({"agents": {"m": {"type": "specialist"}}}),
        json!({"name": "m", "model": "sonnet-4.6"}),
    );
}

#[test]
fn propagate_model_null_clears() {
    propagate_match(
        json!({"agents": {"m": {"type": "specialist", "model": "opus"}}}),
        json!({"name": "m", "model": null}),
    );
}

#[test]
fn propagate_model_agent_missing_returns_null() {
    propagate_match(
        json!({"agents": {"other": {}}}),
        json!({"name": "m", "model": "opus"}),
    );
}

#[test]
fn propagate_model_no_agents_returns_null() {
    propagate_match(
        json!({"version": 1}),
        json!({"name": "m", "model": "opus"}),
    );
}

#[test]
fn propagate_model_seed_missing_errors() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let pa = tmp.path().join("nope.json");
    let pb = tmp.path().join("nope2.json");
    let payload = json!({"name": "m", "model": "opus"});
    let v1 = call_v1("propagateModelToSeed", pa.to_str().unwrap(), &payload).expect_err("v1");
    assert!(v1.contains("não encontrado"));
    let v2 = propagate_model_to_seed(&pb, "m", &json!("opus")).unwrap_err();
    assert!(matches!(
        v2,
        domain_agents::PropagateModelError::SeedMissing(_)
    ));
}
