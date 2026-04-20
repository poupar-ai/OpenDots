//! Paridade v1 (`architect.cjs`) vs v2 — constantes + config.

use domain_architect::{architect_config, architect_context, ARCHITECT_AGENT, ARCHITECT_PROMPT};
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

const NODE_SCRIPT: &str = r#"
const lib = require(process.argv[1]);
const op = process.argv[2];
if (op === 'constants') {
  process.stdout.write(JSON.stringify({
    ARCHITECT_AGENT: lib.ARCHITECT_AGENT,
    ARCHITECT_PROMPT: lib.ARCHITECT_PROMPT,
  }));
} else if (op === 'config') {
  process.stdout.write(JSON.stringify(lib.getArchitectConfig()));
} else if (op === 'context') {
  process.stdout.write(lib.getArchitectContext());
} else {
  process.stderr.write('op desconhecida: ' + op);
  process.exit(2);
}
"#;

fn lib_cjs_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../plugin/scripts/architect.cjs")
        .canonicalize()
        .expect("architect.cjs missing")
}

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn call_v1(op: &str) -> String {
    let out = Command::new("node")
        .arg("-e")
        .arg(NODE_SCRIPT)
        .arg(lib_cjs_path())
        .arg(op)
        .output()
        .expect("spawn node");
    assert!(
        out.status.success(),
        "node falhou: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf8")
}

#[test]
fn constants_match() {
    if !node_available() {
        return;
    }
    let v1_raw = call_v1("constants");
    let v1: Value = serde_json::from_str(&v1_raw).unwrap();
    assert_eq!(v1["ARCHITECT_AGENT"], ARCHITECT_AGENT);
    assert_eq!(v1["ARCHITECT_PROMPT"].as_str().unwrap(), ARCHITECT_PROMPT);
}

#[test]
fn config_matches_byte_for_byte() {
    if !node_available() {
        return;
    }
    let v1 = call_v1("config");
    let v2 = serde_json::to_string(&architect_config()).unwrap();
    assert_eq!(v1, v2);
}

#[test]
fn context_matches() {
    if !node_available() {
        return;
    }
    let v1 = call_v1("context");
    assert_eq!(v1, architect_context());
}
