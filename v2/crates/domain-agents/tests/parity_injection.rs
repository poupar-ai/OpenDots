//! Paridade v1 (`agents.cjs::buildInjectionBlock`) vs v2
//! (`domain_agents::build_injection_block`).
//!
//! ## Importante sobre `context_file`
//!
//! Todo agente nos testes DEVE ter `context_file` (string). Caso
//! contrário o `.cjs` **crasha** com `TypeError` porque
//! `loadMiniContext(memDir, undefined)` chama `path.join(memDir,
//! undefined)` que exige string. A versão Rust é graciosa nesse caso
//! (skipa). Divergência registrada como F3-04.

use domain_agents::{build_injection_block, default_manifest, init_defaults, save_manifest};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

const NODE_SCRIPT: &str = r#"
const lib = require(process.argv[1]);
const memDir = process.argv[2];
const r = lib.buildInjectionBlock(memDir);
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

fn twin_dirs(tmp: &TempDir) -> (PathBuf, PathBuf) {
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();
    (a, b)
}

fn setup_manifest(a: &Path, b: &Path, cfg: &Value) {
    fs::create_dir_all(a.join("agents")).unwrap();
    fs::create_dir_all(b.join("agents")).unwrap();
    save_manifest(a, cfg).unwrap();
    save_manifest(b, cfg).unwrap();
}

fn assert_parity(a: &Path, b: &Path) {
    let v1 = call_v1(a);
    let v2 = build_injection_block(b);
    let v2_str = serde_json::to_string(&v2).unwrap();
    assert_eq!(v1, v2_str, "divergência entre .cjs e Rust");
}

/// Helper: adiciona `context_file` a cada agente do cfg se não tiver,
/// garantindo que `.cjs` não crashe. Mutável para simplicidade.
fn ensure_context_file(cfg: &mut Value) {
    if let Some(agents) = cfg.get_mut("agents").and_then(Value::as_object_mut) {
        for (name, agent) in agents.iter_mut() {
            if let Some(obj) = agent.as_object_mut() {
                obj.entry("context_file")
                    .or_insert(Value::String(format!("agents/{name}.md")));
            }
        }
    }
}

// ─── missing / empty ──────────────────────────────────────────────────────

#[test]
fn parity_missing_manifest_returns_empty() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    assert_parity(&a, &b);
}

#[test]
fn parity_manifest_without_agents_returns_empty() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    setup_manifest(&a, &b, &json!({"version": 1}));
    assert_parity(&a, &b);
}

// ─── default manifest ─────────────────────────────────────────────────────

#[test]
fn parity_default_manifest_without_mini_contexts() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    setup_manifest(&a, &b, &default_manifest());
    assert_parity(&a, &b);
}

#[test]
fn parity_default_manifest_with_mini_contexts() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let script = r#"
const lib = require(process.argv[1]);
lib.initDefaults(process.argv[2], { overwrite: false });
"#;
    let out = Command::new("node")
        .arg("-e")
        .arg(script)
        .arg(lib_cjs_path())
        .arg(&a)
        .output()
        .unwrap();
    assert!(out.status.success());
    init_defaults(&b, false).unwrap();

    assert_parity(&a, &b);
}

// ─── edge cases ───────────────────────────────────────────────────────────

#[test]
fn parity_no_rule_field() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let mut cfg = json!({
        "version": 1,
        "manager": "m",
        "agents": {
            "m": {"type": "core", "role": "role manager", "emoji": "🎯"}
        }
    });
    ensure_context_file(&mut cfg);
    setup_manifest(&a, &b, &cfg);
    assert_parity(&a, &b);
}

#[test]
fn parity_empty_rule_string_skipped() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let mut cfg = json!({
        "version": 1,
        "manager": "m",
        "rule": "",
        "agents": {"m": {"type": "core", "role": "x"}}
    });
    ensure_context_file(&mut cfg);
    setup_manifest(&a, &b, &cfg);
    assert_parity(&a, &b);
}

#[test]
fn parity_unknown_type_falls_to_specialist() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let mut cfg = json!({
        "version": 1,
        "manager": "m",
        "agents": {
            "m": {"type": "core", "role": "x"},
            "weirdo": {"type": "ghost", "role": "unknown"}
        }
    });
    ensure_context_file(&mut cfg);
    setup_manifest(&a, &b, &cfg);
    assert_parity(&a, &b);
}

#[test]
fn parity_firefighter_with_watch_full() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let mut cfg = json!({
        "version": 1,
        "manager": "m",
        "agents": {
            "m": {"type": "core", "role": "x"},
            "smoke": {
                "type": "firefighter",
                "role": "watch logs",
                "watch": {
                    "source": "datadog",
                    "query": "service:api status:error",
                    "threshold": 10,
                    "window_minutes": 15
                }
            }
        }
    });
    ensure_context_file(&mut cfg);
    setup_manifest(&a, &b, &cfg);
    assert_parity(&a, &b);
}

#[test]
fn parity_firefighter_watch_without_query_skipped() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let mut cfg = json!({
        "version": 1,
        "manager": "m",
        "agents": {
            "m": {"type": "core", "role": "x"},
            "smoke": {
                "type": "firefighter",
                "role": "no query",
                "watch": {"source": "datadog", "threshold": 5}
            }
        }
    });
    ensure_context_file(&mut cfg);
    setup_manifest(&a, &b, &cfg);
    assert_parity(&a, &b);
}

#[test]
fn parity_mcps_rendered_when_non_empty() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let mut cfg = json!({
        "version": 1,
        "manager": "m",
        "agents": {
            "m": {"type": "core", "role": "x"},
            "bot": {"type": "motor", "role": "act", "mcps": ["stripe", "sendgrid"]}
        }
    });
    ensure_context_file(&mut cfg);
    setup_manifest(&a, &b, &cfg);
    assert_parity(&a, &b);
}

#[test]
fn parity_mcps_empty_not_rendered() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let mut cfg = json!({
        "version": 1,
        "manager": "m",
        "agents": {
            "m": {"type": "core", "role": "x"},
            "bot": {"type": "motor", "role": "act", "mcps": []}
        }
    });
    ensure_context_file(&mut cfg);
    setup_manifest(&a, &b, &cfg);
    assert_parity(&a, &b);
}

#[test]
fn parity_can_talk_to_rendered() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let mut cfg = json!({
        "version": 1,
        "manager": "m",
        "agents": {
            "m": {"type": "core", "role": "x", "can_talk_to": ["oracle", "architect"]}
        }
    });
    ensure_context_file(&mut cfg);
    setup_manifest(&a, &b, &cfg);
    assert_parity(&a, &b);
}

#[test]
fn parity_missing_role_empty_string() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let mut cfg = json!({
        "version": 1,
        "manager": "m",
        "agents": {
            "m": {"type": "core"}
        }
    });
    ensure_context_file(&mut cfg);
    setup_manifest(&a, &b, &cfg);
    assert_parity(&a, &b);
}

#[test]
fn parity_custom_emoji() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let mut cfg = json!({
        "version": 1,
        "manager": "m",
        "agents": {
            "m": {"type": "core", "role": "x", "emoji": "🦀"}
        }
    });
    ensure_context_file(&mut cfg);
    setup_manifest(&a, &b, &cfg);
    assert_parity(&a, &b);
}

#[test]
fn parity_no_emoji_uses_bullet() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let mut cfg = json!({
        "version": 1,
        "manager": "m",
        "agents": {
            "m": {"type": "core", "role": "x"}
        }
    });
    ensure_context_file(&mut cfg);
    setup_manifest(&a, &b, &cfg);
    assert_parity(&a, &b);
}

#[test]
fn parity_mini_context_snippet() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let cfg = json!({
        "version": 1,
        "manager": "m",
        "agents": {
            "m": {"type": "core", "role": "x", "context_file": "agents/m.md"}
        }
    });
    setup_manifest(&a, &b, &cfg);
    let md = "# Titulo\n\n## Responsabilidade\nSou o manager principal do time.\n\n## Princípios\n- Um só.";
    fs::write(a.join("agents/m.md"), md).unwrap();
    fs::write(b.join("agents/m.md"), md).unwrap();
    assert_parity(&a, &b);
}

#[test]
fn parity_mini_context_snippet_truncated_at_140() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let cfg = json!({
        "version": 1,
        "manager": "m",
        "agents": {
            "m": {"type": "core", "role": "x", "context_file": "agents/m.md"}
        }
    });
    setup_manifest(&a, &b, &cfg);
    let md = format!("# Heading\n{}", "a".repeat(300));
    fs::write(a.join("agents/m.md"), &md).unwrap();
    fs::write(b.join("agents/m.md"), &md).unwrap();
    assert_parity(&a, &b);
}

#[test]
fn parity_mini_context_missing_file_no_snippet() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let cfg = json!({
        "version": 1,
        "manager": "m",
        "agents": {
            "m": {"type": "core", "role": "x", "context_file": "agents/missing.md"}
        }
    });
    setup_manifest(&a, &b, &cfg);
    assert_parity(&a, &b);
}

#[test]
fn parity_manager_tag_custom_name() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let mut cfg = json!({
        "version": 1,
        "manager": "boss",
        "agents": {
            "boss": {"type": "core", "role": "lead"},
            "helper": {"type": "specialist", "role": "assist"}
        }
    });
    ensure_context_file(&mut cfg);
    setup_manifest(&a, &b, &cfg);
    assert_parity(&a, &b);
}

#[test]
fn parity_oracle_tag() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let mut cfg = json!({
        "version": 1,
        "manager": "m",
        "oracle": "seer",
        "agents": {
            "m": {"type": "core", "role": "lead"},
            "seer": {"type": "core", "role": "know"}
        }
    });
    ensure_context_file(&mut cfg);
    setup_manifest(&a, &b, &cfg);
    assert_parity(&a, &b);
}

#[test]
fn parity_all_group_types_present() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let mut cfg = json!({
        "version": 1,
        "manager": "m",
        "agents": {
            "m":    {"type": "core", "role": "c"},
            "s":    {"type": "specialist", "role": "s"},
            "i":    {"type": "integrator", "role": "i"},
            "mot":  {"type": "motor", "role": "mo"},
            "fire": {"type": "firefighter", "role": "f"}
        }
    });
    ensure_context_file(&mut cfg);
    setup_manifest(&a, &b, &cfg);
    assert_parity(&a, &b);
}

// ─── F3-04 — Rust é mais robusta que .cjs ─────────────────────────────────

#[test]
fn rust_does_not_crash_on_missing_context_file() {
    // .cjs crasha aqui. Rust gracioso.
    let tmp = TempDir::new().unwrap();
    let cfg = json!({
        "version": 1,
        "manager": "m",
        "agents": {
            "m": {"type": "core", "role": "x"}
        }
    });
    fs::create_dir_all(tmp.path().join("agents")).unwrap();
    save_manifest(tmp.path(), &cfg).unwrap();

    // Não deve crashar.
    let lines = build_injection_block(tmp.path());
    assert!(lines.iter().any(|l| l.contains("**m**")));
}
