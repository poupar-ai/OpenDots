//! Paridade v1 (`destructive.cjs::detect`) vs v2.
//!
//! Foco: bateria de inputs PT/EN/shell/SQL, verificar que as duas
//! engines de regex chegam ao MESMO resultado (matched + hits).

use domain_destructive::{detect, save_user_patterns, set_core_disabled, FixedIdGen, FixedIsoClock};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::TempDir;

const NODE_SCRIPT: &str = r#"
const cjsPath = process.argv[1];
const lib = require(cjsPath);
let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', d => { input += d; });
process.stdin.on('end', () => {
  try {
    const { op, memDir, args } = JSON.parse(input);
    let out;
    if (op === 'detect') out = lib.detect(memDir, args.text);
    else if (op === 'listPatterns') out = lib.listPatterns(memDir);
    else throw new Error('unknown op ' + op);
    process.stdout.write(JSON.stringify(out === undefined ? null : out));
  } catch (e) {
    process.stderr.write('THROW: ' + e.message);
    process.exit(3);
  }
});
"#;

fn cjs_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../plugin/scripts/destructive.cjs")
        .canonicalize()
        .expect("destructive.cjs missing")
}

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_node(mem_dir: &Path, op: &str, args: Value) -> Value {
    let input = json!({"op": op, "memDir": mem_dir.to_string_lossy(), "args": args});
    let mut child = Command::new("node")
        .arg("-e")
        .arg(NODE_SCRIPT)
        .arg(cjs_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn node");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().expect("wait node");
    if !out.status.success() {
        panic!(
            "node exited {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    serde_json::from_slice(&out.stdout).expect("parse node stdout")
}

fn assert_parity(text: &str) {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let v1 = run_node(a.path(), "detect", json!({"text": text}));
    let v2 = detect(b.path(), text).to_json();
    assert_eq!(v1, v2, "diverge on input: {text:?}");
}

// ── Sanity: strings benignas ─────────────────────────────────────────────

#[test]
fn empty_string() {
    assert_parity("");
}

#[test]
fn plain_text() {
    assert_parity("hello world, nothing destructive here");
}

#[test]
fn greeting_pt() {
    assert_parity("bom dia, tudo bem?");
}

// ── CORE: rm / git / SQL ─────────────────────────────────────────────────

#[test]
fn rm_rf_simple() {
    assert_parity("rm -rf /tmp/foo");
}

#[test]
fn rm_rf_various_flags() {
    assert_parity("please do: rm -fr /var && rm -Rf /etc");
}

#[test]
fn git_reset_hard() {
    assert_parity("git reset --hard HEAD~3");
}

#[test]
fn git_clean_force() {
    assert_parity("git clean -fdx");
}

#[test]
fn git_push_force() {
    assert_parity("git push --force origin main");
}

#[test]
fn git_branch_delete() {
    assert_parity("git branch -D feature/foo");
}

#[test]
fn drop_database() {
    assert_parity("DROP DATABASE prod;");
}

#[test]
fn drop_table() {
    assert_parity("drop table users cascade");
}

#[test]
fn truncate_table() {
    assert_parity("TRUNCATE TABLE orders");
}

#[test]
fn delete_from_no_where() {
    assert_parity("DELETE FROM users;");
}

#[test]
fn delete_from_with_where_ok() {
    // Com WHERE, não deve bater (regex exige `;` ou `$` após \w+).
    assert_parity("DELETE FROM users WHERE id = 1");
}

#[test]
fn format_disk() {
    assert_parity("mkfs /dev/sda1");
}

#[test]
fn dd_of_dev() {
    assert_parity("dd if=/dev/zero of=/dev/sda bs=1M");
}

#[test]
fn chmod_777_root() {
    assert_parity("chmod -R 777 /");
}

#[test]
fn shutdown() {
    assert_parity("shutdown -h now");
}

// ── CORE: PT patterns ────────────────────────────────────────────────────

#[test]
fn apagar_tudo_pt() {
    assert_parity("apague tudo agora");
}

#[test]
fn apagar_o_projeto() {
    assert_parity("por favor delete o projeto inteiro");
}

#[test]
fn apagar_reposito_acento() {
    assert_parity("quero apagar o repositório");
}

#[test]
fn apagar_base_de_dados() {
    assert_parity("preciso remover a base de dados de produção");
}

#[test]
fn apagar_as_memorias() {
    assert_parity("limpe as memórias do projeto");
}

// ── CORE: EN patterns ────────────────────────────────────────────────────

#[test]
fn delete_everything_en() {
    assert_parity("please delete everything in the repo");
}

#[test]
fn nuke_the_codebase() {
    assert_parity("nuke the codebase");
}

#[test]
fn wipe_all_data() {
    assert_parity("wipe all data from the branch");
}

// ── Múltiplos hits + ordem ───────────────────────────────────────────────

#[test]
fn multiple_hits_order_preserved() {
    assert_parity(
        "rm -rf / && DROP DATABASE prod; and delete everything in the repository",
    );
}

// ── Disabled CORE ────────────────────────────────────────────────────────

#[test]
fn disabled_core_not_triggered() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    // Desabilita mesmo pattern nos dois.
    let lib_cjs = cjs_path();
    let _ = run_disable(a.path(), "core.rm_rf", true, &lib_cjs);
    set_core_disabled(b.path(), "core.rm_rf", true).unwrap();

    let v1 = run_node(a.path(), "detect", json!({"text": "rm -rf /tmp"}));
    let v2 = detect(b.path(), "rm -rf /tmp").to_json();
    assert_eq!(v1, v2);
    assert_eq!(v2["matched"], false);
}

fn run_disable(mem_dir: &Path, id: &str, disabled: bool, cjs: &Path) -> Value {
    let script = r#"
const cjsPath = process.argv[1];
const lib = require(cjsPath);
let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', d => { input += d; });
process.stdin.on('end', () => {
  const { memDir, id, disabled } = JSON.parse(input);
  const r = lib.setCoreDisabled(memDir, id, disabled);
  process.stdout.write(JSON.stringify(r));
});
"#;
    let input = json!({"memDir": mem_dir.to_string_lossy(), "id": id, "disabled": disabled});
    let mut child = Command::new("node")
        .arg("-e")
        .arg(script)
        .arg(cjs)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    serde_json::from_slice(&out.stdout).unwrap()
}

// ── User patterns ────────────────────────────────────────────────────────

#[test]
fn user_pattern_literal_matches_both() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    // Seed o arquivo manualmente nos dois — pulando normalização (pra
    // garantir created_at idêntico).
    let seed = json!({
        "patterns": [{
            "id": "user.abc",
            "label": "Block prod wipe",
            "text": "wipe prod",
            "kind": "literal",
            "severity": "critical",
            "enabled": true,
            "created_at": "2026-04-19T12:00:00.000Z",
        }],
        "disabled_core": [],
    });
    let content = serde_json::to_string_pretty(&seed).unwrap();
    std::fs::write(a.path().join("destructive_patterns.json"), &content).unwrap();
    std::fs::write(b.path().join("destructive_patterns.json"), &content).unwrap();

    let text = "please wipe prod right now";
    let v1 = run_node(a.path(), "detect", json!({"text": text}));
    let v2 = detect(b.path(), text).to_json();
    assert_eq!(v1, v2);
    assert_eq!(v2["matched"], true);
}

#[test]
fn user_pattern_wildcard_star() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let seed = json!({
        "patterns": [{
            "id": "user.kw",
            "label": "kill-*",
            "text": "kill-*",
            "kind": "literal",
            "severity": "high",
            "enabled": true,
            "created_at": "2026-04-19T12:00:00.000Z",
        }],
        "disabled_core": [],
    });
    let content = serde_json::to_string_pretty(&seed).unwrap();
    std::fs::write(a.path().join("destructive_patterns.json"), &content).unwrap();
    std::fs::write(b.path().join("destructive_patterns.json"), &content).unwrap();

    let text = "please run kill-everything now";
    let v1 = run_node(a.path(), "detect", json!({"text": text}));
    let v2 = detect(b.path(), text).to_json();
    assert_eq!(v1, v2);
    assert_eq!(v2["matched"], true);
}

#[test]
fn user_pattern_regex_kind() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let seed = json!({
        "patterns": [{
            "id": "user.re",
            "label": "kubectl delete ns",
            "text": "kubectl\\s+delete\\s+ns",
            "kind": "regex",
            "severity": "high",
            "enabled": true,
            "created_at": "2026-04-19T12:00:00.000Z",
        }],
        "disabled_core": [],
    });
    let content = serde_json::to_string_pretty(&seed).unwrap();
    std::fs::write(a.path().join("destructive_patterns.json"), &content).unwrap();
    std::fs::write(b.path().join("destructive_patterns.json"), &content).unwrap();

    let text = "kubectl delete ns prod";
    let v1 = run_node(a.path(), "detect", json!({"text": text}));
    let v2 = detect(b.path(), text).to_json();
    assert_eq!(v1, v2);
}

#[test]
fn user_pattern_disabled_not_applied() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let seed = json!({
        "patterns": [{
            "id": "user.off",
            "label": "off",
            "text": "mydangerous",
            "kind": "literal",
            "severity": "high",
            "enabled": false,
            "created_at": "2026-04-19T12:00:00.000Z",
        }],
        "disabled_core": [],
    });
    let content = serde_json::to_string_pretty(&seed).unwrap();
    std::fs::write(a.path().join("destructive_patterns.json"), &content).unwrap();
    std::fs::write(b.path().join("destructive_patterns.json"), &content).unwrap();

    let text = "running mydangerous thing";
    let v1 = run_node(a.path(), "detect", json!({"text": text}));
    let v2 = detect(b.path(), text).to_json();
    assert_eq!(v1, v2);
    assert_eq!(v2["matched"], false);
}

#[test]
fn save_then_detect_roundtrip() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();

    // Gerar save via v2 com id/clock fixos; depois também no Node
    // semeando arquivo idêntico (não chamando saveUserPatterns pra
    // evitar diff no created_at).
    let id_gen = FixedIdGen("user.fixedid".into());
    let clock = FixedIsoClock("2026-04-19T12:00:00.000Z".into());
    let input = vec![json!({
        "label": "Block wipe prod",
        "text": "wipe prod",
        "severity": "critical",
    })];
    save_user_patterns(b.path(), &input, &id_gen, &clock);
    // Copia o arquivo salvo de b → a pra garantir paridade de seed.
    let content = std::fs::read(b.path().join("destructive_patterns.json")).unwrap();
    std::fs::write(a.path().join("destructive_patterns.json"), content).unwrap();

    let text = "want to wipe prod db";
    let v1 = run_node(a.path(), "detect", json!({"text": text}));
    let v2 = detect(b.path(), text).to_json();
    assert_eq!(v1, v2);
}

// ── listPatterns ─────────────────────────────────────────────────────────

#[test]
fn list_patterns_empty() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let v1 = run_node(a.path(), "listPatterns", json!({}));
    let v2 = domain_destructive::list_patterns(a.path());
    assert_eq!(v1, v2);
    assert_eq!(v2["core"].as_array().unwrap().len(), 14);
    assert_eq!(v2["user"].as_array().unwrap().len(), 0);
}

#[test]
fn list_patterns_with_user() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let seed = json!({
        "patterns": [{
            "id": "user.one",
            "label": "one",
            "text": "trigger",
            "kind": "literal",
            "severity": "high",
            "enabled": true,
            "created_at": "2026-04-19T12:00:00.000Z",
        }],
        "disabled_core": ["core.shutdown"],
    });
    let content = serde_json::to_string_pretty(&seed).unwrap();
    std::fs::write(a.path().join("destructive_patterns.json"), &content).unwrap();
    std::fs::write(b.path().join("destructive_patterns.json"), &content).unwrap();

    let v1 = run_node(a.path(), "listPatterns", json!({}));
    let v2 = domain_destructive::list_patterns(b.path());
    assert_eq!(v1, v2);
}
