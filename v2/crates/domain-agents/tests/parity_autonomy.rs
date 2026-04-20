//! Paridade v1 (`agents.cjs::normalizeAutonomy`/`updateAutonomy`/
//! `propagateAutonomyToSeed`) vs v2 (`domain_agents::autonomy::*`).

use domain_agents::{
    normalize_autonomy, propagate_autonomy_to_seed, save_manifest, update_autonomy,
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
    if (op === 'normalizeAutonomy') {
      const r = lib.normalizeAutonomy(payload);
      out = r === null ? 'NULL' : JSON.stringify(r);
    } else if (op === 'updateAutonomy') {
      out = JSON.stringify(lib.updateAutonomy(arg, payload.name, payload.autonomy));
    } else if (op === 'propagateAutonomyToSeed') {
      const r = lib.propagateAutonomyToSeed(arg, payload.name, payload.autonomy);
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

// ── normalize_autonomy ─────────────────────────────────────────────────────

fn norm_match(input: Value) {
    if !node_available() {
        return;
    }
    let v1 = call_v1("normalizeAutonomy", "", &input).expect("v1 ok");
    let v2 = normalize_autonomy(&input).expect("v2 ok");
    let v2_str = match v2 {
        None => "NULL".to_string(),
        Some(v) => serde_json::to_string(&v).unwrap(),
    };
    assert_eq!(v1, v2_str, "input={input}");
}

#[test]
fn normalize_null() {
    norm_match(json!(null));
}

#[test]
fn normalize_low() {
    norm_match(json!({"level": "low"}));
}

#[test]
fn normalize_medium_with_call_human() {
    norm_match(json!({
        "level": "medium",
        "call_human_when": ["pagar", "mover dinheiro"]
    }));
}

#[test]
fn normalize_high_all_filters() {
    norm_match(json!({
        "level": "high",
        "call_human_when": [" escala ", "", 42, null, "  ", "ok", true]
    }));
}

#[test]
fn normalize_key_order_regardless_of_input() {
    norm_match(json!({
        "call_human_when": ["a"],
        "level": "low"
    }));
}

#[test]
fn normalize_call_human_non_array_becomes_empty() {
    norm_match(json!({"level": "high", "call_human_when": "solo"}));
    norm_match(json!({"level": "high", "call_human_when": null}));
    norm_match(json!({"level": "high", "call_human_when": 42}));
}

#[test]
fn normalize_extra_keys_ignored() {
    norm_match(json!({"level": "low", "call_human_when": [], "extra": "x"}));
}

// ── errors em normalize ────────────────────────────────────────────────────

#[test]
fn normalize_errors_not_object() {
    if !node_available() {
        return;
    }
    for input in [json!("x"), json!(42), json!(true), json!(false)] {
        let v1 = call_v1("normalizeAutonomy", "", &input).expect_err("v1 deve lançar");
        assert!(v1.contains("precisa ser objeto") || v1.contains("inválido"), "v1={v1}");
        assert!(
            normalize_autonomy(&input).is_err(),
            "v2 deve erro pra {input}"
        );
    }
}

#[test]
fn normalize_errors_invalid_level() {
    if !node_available() {
        return;
    }
    for input in [
        json!({}),
        json!({"level": "xyz"}),
        json!({"level": "HIGH"}),
        json!({"level": null}),
    ] {
        let v1 = call_v1("normalizeAutonomy", "", &input).expect_err("v1");
        assert!(v1.contains("level inválido"), "v1={v1}");
        assert!(
            normalize_autonomy(&input).is_err(),
            "v2 deve erro pra {input}"
        );
    }
}

// ── update_autonomy ────────────────────────────────────────────────────────

fn setup_manifest(dir: &Path, name: &str) {
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
    setup_manifest(&a, name);
    setup_manifest(&b, name);

    let v1 = call_v1(
        "updateAutonomy",
        a.to_str().unwrap(),
        &payload,
    )
    .expect("v1 ok");
    let v2 = update_autonomy(&b, name, &payload["autonomy"]).expect("v2 ok");

    assert_eq!(v1, v2.to_string());
    let ja = fs::read(a.join("agents.json")).unwrap();
    let jb = fs::read(b.join("agents.json")).unwrap();
    assert_eq!(ja, jb, "agents.json difere");
}

#[test]
fn update_autonomy_sets_low() {
    update_match(json!({"name": "m", "autonomy": {"level": "low"}}));
}

#[test]
fn update_autonomy_full() {
    update_match(json!({
        "name": "m",
        "autonomy": {"level": "high", "call_human_when": ["escala crítica"]}
    }));
}

#[test]
fn update_autonomy_null_clears() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let manifest = json!({
        "version": 1,
        "agents": {"m": {
            "type": "specialist",
            "autonomy": {"level": "low", "call_human_when": []}
        }}
    });
    save_manifest(&a, &manifest).unwrap();
    save_manifest(&b, &manifest).unwrap();

    let payload = json!({"name": "m", "autonomy": null});
    let v1 = call_v1("updateAutonomy", a.to_str().unwrap(), &payload).expect("v1 ok");
    let v2 = update_autonomy(&b, "m", &json!(null)).expect("v2 ok");

    assert_eq!(v1, v2.to_string());
    let ja = fs::read(a.join("agents.json")).unwrap();
    let jb = fs::read(b.join("agents.json")).unwrap();
    assert_eq!(ja, jb);
}

#[test]
fn update_autonomy_agent_missing_errors() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    save_manifest(&a, &json!({"version":1,"agents":{}})).unwrap();
    save_manifest(&b, &json!({"version":1,"agents":{}})).unwrap();

    let payload = json!({"name": "missing", "autonomy": {"level": "low"}});
    let v1 = call_v1("updateAutonomy", a.to_str().unwrap(), &payload).expect_err("v1");
    assert!(v1.contains("não existe"));
    let v2 = update_autonomy(&b, "missing", &json!({"level": "low"})).unwrap_err();
    assert!(matches!(
        v2,
        domain_agents::UpdateAutonomyError::AgentMissing { .. }
    ));
}

// ── propagate_autonomy_to_seed ─────────────────────────────────────────────

fn propagate_match(initial_seed: Value, payload: Value) {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let pa = tmp.path().join("a.json");
    let pb = tmp.path().join("b.json");
    let initial = serde_json::to_string_pretty(&initial_seed).unwrap() + "\n";
    fs::write(&pa, &initial).unwrap();
    fs::write(&pb, &initial).unwrap();

    let v1 = call_v1(
        "propagateAutonomyToSeed",
        pa.to_str().unwrap(),
        &payload,
    )
    .expect("v1 ok");
    let v2 = propagate_autonomy_to_seed(&pb, payload["name"].as_str().unwrap(), &payload["autonomy"])
        .expect("v2 ok");

    let v2_str = match v2 {
        None => "NULL".to_string(),
        Some(v) => serde_json::to_string(&v).unwrap(),
    };
    assert_eq!(v1, v2_str, "retorno difere");

    // bytes idênticos no seed
    let ba = fs::read(&pa).unwrap();
    let bb = fs::read(&pb).unwrap();
    assert_eq!(
        ba, bb,
        "seed.json difere:\nv1={}\nv2={}",
        String::from_utf8_lossy(&ba),
        String::from_utf8_lossy(&bb)
    );
}

#[test]
fn propagate_sets_autonomy() {
    propagate_match(
        json!({"agents": {"m": {"type": "specialist"}}}),
        json!({"name": "m", "autonomy": {"level": "high", "call_human_when": ["x"]}}),
    );
}

#[test]
fn propagate_null_removes() {
    propagate_match(
        json!({"agents": {"m": {"type": "specialist", "autonomy": {"level": "low", "call_human_when": []}}}}),
        json!({"name": "m", "autonomy": null}),
    );
}

#[test]
fn propagate_no_agent_returns_null() {
    propagate_match(
        json!({"agents": {"other": {}}}),
        json!({"name": "m", "autonomy": {"level": "low"}}),
    );
}

#[test]
fn propagate_no_agents_key_returns_null() {
    propagate_match(
        json!({"version": 1}),
        json!({"name": "m", "autonomy": {"level": "low"}}),
    );
}

#[test]
fn propagate_preserves_other_agents() {
    propagate_match(
        json!({
            "agents": {
                "other": {"type": "specialist", "role": "keep me"},
                "m": {"type": "specialist"}
            }
        }),
        json!({"name": "m", "autonomy": {"level": "medium"}}),
    );
}

#[test]
fn propagate_errors_when_seed_missing() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let pa = tmp.path().join("nope.json");
    let pb = tmp.path().join("nope2.json");
    let payload = json!({"name": "m", "autonomy": {"level": "low"}});
    let v1 = call_v1(
        "propagateAutonomyToSeed",
        pa.to_str().unwrap(),
        &payload,
    )
    .expect_err("v1");
    assert!(v1.contains("não encontrado"));
    let v2 = propagate_autonomy_to_seed(&pb, "m", &json!({"level": "low"})).unwrap_err();
    assert!(matches!(
        v2,
        domain_agents::PropagateAutonomyError::SeedMissing(_)
    ));
}
