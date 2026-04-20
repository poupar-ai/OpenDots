//! Paridade v1 (`plugin/scripts/agents.cjs`) vs v2 (`domain_agents`) —
//! sub-ponto A: constantes exportadas e path helpers.
//!
//! Para o `.cjs`, `AGENT_TYPES` e `AUTONOMY_LEVELS` são `Set<string>`.
//! Aqui são `&[&str]`. Paridade verificada transformando o `Set` em
//! array ordenado (Node preserva insertion order em Sets) e comparando
//! com o slice Rust.

use domain_agents::{
    agents_dir, is_valid_agent_type, is_valid_autonomy_level, manifest_path, AGENT_TYPES,
    AUTONOMY_LEVELS, DIR, MANIFEST,
};
use std::path::{Path, PathBuf};
use std::process::Command;

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

fn run_node(script: &str) -> String {
    let out = Command::new("node")
        .arg("-e")
        .arg(script)
        .arg(lib_cjs_path())
        .output()
        .expect("spawn node");
    assert!(
        out.status.success(),
        "node falhou: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf8")
}

// ─── constantes de string ──────────────────────────────────────────────────

#[test]
fn parity_manifest_constant() {
    // Não precisa de node — só confere o valor.
    assert_eq!(MANIFEST, "agents.json");
}

#[test]
fn parity_dir_constant() {
    assert_eq!(DIR, "agents");
}

// ─── AGENT_TYPES ──────────────────────────────────────────────────────────

#[test]
fn parity_agent_types_exact_contents() {
    if !node_available() {
        return;
    }
    let got = run_node(
        r#"const m = require(process.argv[1]);
           process.stdout.write(JSON.stringify([...m.AGENT_TYPES]));"#,
    );
    let cjs: Vec<String> = serde_json::from_str(&got).expect("parse");
    let rust: Vec<String> = AGENT_TYPES.iter().map(|s| s.to_string()).collect();
    assert_eq!(cjs, rust);
}

#[test]
fn parity_agent_types_has_semantics() {
    if !node_available() {
        return;
    }
    // Valida `.has(x)` em ambos (para todos os elementos + alguns rejeitos).
    for candidate in ["core", "specialist", "integrator", "motor", "firefighter", "unknown", ""] {
        let script = format!(
            r#"const m = require(process.argv[1]);
               process.stdout.write(String(m.AGENT_TYPES.has({:?})));"#,
            candidate
        );
        let cjs = run_node(&script) == "true";
        let rust = is_valid_agent_type(candidate);
        assert_eq!(cjs, rust, "candidate={candidate}");
    }
}

// ─── AUTONOMY_LEVELS ──────────────────────────────────────────────────────

#[test]
fn parity_autonomy_levels_exact_contents() {
    if !node_available() {
        return;
    }
    let got = run_node(
        r#"const m = require(process.argv[1]);
           process.stdout.write(JSON.stringify([...m.AUTONOMY_LEVELS]));"#,
    );
    let cjs: Vec<String> = serde_json::from_str(&got).expect("parse");
    let rust: Vec<String> = AUTONOMY_LEVELS.iter().map(|s| s.to_string()).collect();
    assert_eq!(cjs, rust);
}

#[test]
fn parity_autonomy_levels_has_semantics() {
    if !node_available() {
        return;
    }
    for candidate in ["low", "medium", "high", "auto", "", "HIGH"] {
        let script = format!(
            r#"const m = require(process.argv[1]);
               process.stdout.write(String(m.AUTONOMY_LEVELS.has({:?})));"#,
            candidate
        );
        let cjs = run_node(&script) == "true";
        let rust = is_valid_autonomy_level(candidate);
        assert_eq!(cjs, rust, "candidate={candidate}");
    }
}

// ─── manifest_path / agents_dir ───────────────────────────────────────────

#[test]
fn parity_manifest_path_simple() {
    if !node_available() {
        return;
    }
    let got = run_node(
        r#"const m = require(process.argv[1]);
           process.stdout.write(m.manifestPath('/m'));"#,
    );
    let rust = manifest_path(Path::new("/m"));
    assert_eq!(got, rust.to_string_lossy());
}

#[test]
fn parity_agents_dir_simple() {
    if !node_available() {
        return;
    }
    let got = run_node(
        r#"const m = require(process.argv[1]);
           process.stdout.write(m.agentsDir('/m'));"#,
    );
    let rust = agents_dir(Path::new("/m"));
    assert_eq!(got, rust.to_string_lossy());
}

#[test]
fn parity_manifest_path_with_trailing_slash() {
    if !node_available() {
        return;
    }
    let got = run_node(
        r#"const m = require(process.argv[1]);
           process.stdout.write(m.manifestPath('/m/'));"#,
    );
    // path.join('/m/', 'agents.json') → '/m/agents.json' (Node normaliza).
    // Rust PathBuf::join('/m/').join('agents.json') → '/m/agents.json'
    // também (Rust considera '/m/' sem segmento trailing).
    let rust = manifest_path(Path::new("/m/"));
    assert_eq!(got, rust.to_string_lossy());
}

#[test]
fn parity_agents_dir_nested() {
    if !node_available() {
        return;
    }
    let got = run_node(
        r#"const m = require(process.argv[1]);
           process.stdout.write(m.agentsDir('/var/mem/.claude-project-memory'));"#,
    );
    let rust = agents_dir(Path::new("/var/mem/.claude-project-memory"));
    assert_eq!(got, rust.to_string_lossy());
}
