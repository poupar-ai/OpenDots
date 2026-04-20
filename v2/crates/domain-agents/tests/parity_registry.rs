//! Paridade v1 (`agents.cjs::addAgent`/`removeAgent`) vs v2
//! (`domain_agents::add_agent`/`remove_agent`).
//!
//! Estratégia:
//! - Dois dirs irmãos (a, b); v1 roda no dir `a` via node, v2 no `b`.
//! - Comparação byte-a-byte do `agents.json` e do mini-context (`.md`).
//! - Input vai por stdin como JSON pra não estourar argv.

use domain_agents::{add_agent, remove_agent};
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::TempDir;

const NODE_SCRIPT: &str = r#"
const lib = require(process.argv[1]);
const op = process.argv[2];
const memDir = process.argv[3];

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', d => { input += d; });
process.stdin.on('end', () => {
  const payload = input ? JSON.parse(input) : null;
  try {
    let out;
    if (op === 'addAgent') {
      out = JSON.stringify(lib.addAgent(memDir, payload.name, payload.config));
    } else if (op === 'removeAgent') {
      out = JSON.stringify(lib.removeAgent(memDir, payload.name));
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

fn twin_dirs(tmp: &TempDir) -> (PathBuf, PathBuf) {
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();
    (a, b)
}

fn assert_agents_json_match(a: &Path, b: &Path) {
    let ja = fs::read(a.join("agents.json")).unwrap();
    let jb = fs::read(b.join("agents.json")).unwrap();
    assert_eq!(
        ja, jb,
        "agents.json difere:\nv1={}\nv2={}",
        String::from_utf8_lossy(&ja),
        String::from_utf8_lossy(&jb)
    );
}

fn assert_ctx_match(a: &Path, b: &Path, rel: &str) {
    let ca = fs::read(a.join(rel)).unwrap();
    let cb = fs::read(b.join(rel)).unwrap();
    assert_eq!(
        ca, cb,
        "{rel} difere:\nv1={}\nv2={}",
        String::from_utf8_lossy(&ca),
        String::from_utf8_lossy(&cb)
    );
}

// ── add_agent ──────────────────────────────────────────────────────────────

fn add_match(name: &str, config: Value) {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);

    let payload = json!({"name": name, "config": config});
    let v1 = call_v1("addAgent", &a, &payload).expect("v1 ok");
    let v2 = add_agent(&b, name, &config).expect("v2 ok");

    assert_eq!(v1, v2.to_string(), "retorno difere (name={name})");
    assert_agents_json_match(&a, &b);

    let ctx_file = v2["context_file"].as_str().unwrap();
    if a.join(ctx_file).exists() {
        assert_ctx_match(&a, &b, ctx_file);
    }
}

#[test]
fn add_agent_minimal_specialist() {
    add_match("foo", json!({"type": "specialist"}));
}

#[test]
fn add_agent_full_specialist() {
    add_match(
        "reviewer",
        json!({
            "type": "specialist",
            "emoji": "🔍",
            "model": "opus-4.7",
            "role": "Reviewer de código",
            "tools": ["Read", "Grep"],
            "mcps": [],
            "can_talk_to": ["manager", "oracle"],
            "scope": {"bash_allow": []}
        }),
    );
}

#[test]
fn add_agent_integrator_default_emoji() {
    add_match("gh", json!({"type": "integrator"}));
}

#[test]
fn add_agent_motor_default_emoji() {
    add_match("shipper", json!({"type": "motor"}));
}

#[test]
fn add_agent_firefighter_default_emoji() {
    add_match("alerta", json!({"type": "firefighter"}));
}

#[test]
fn add_agent_gender_female() {
    add_match(
        "ana",
        json!({"type": "specialist", "gender": "female", "role": "Analista"}),
    );
}

#[test]
fn add_agent_gender_invalid_ignored() {
    add_match(
        "x",
        json!({"type": "specialist", "gender": "fluid"}),
    );
}

#[test]
fn add_agent_with_context_body() {
    add_match(
        "foo",
        json!({
            "type": "specialist",
            "context": "# Foo\n\nConteúdo custom do mini-context.\n"
        }),
    );
}

#[test]
fn add_agent_custom_context_file_path() {
    add_match(
        "foo",
        json!({
            "type": "specialist",
            "context_file": "custom/foo.md",
            "context": "body"
        }),
    );
}

#[test]
fn add_agent_firefighter_with_watch() {
    add_match(
        "alerta",
        json!({
            "type": "firefighter",
            "watch": {
                "source": "datadog",
                "query": "service:api errors",
                "threshold": 10,
                "window_minutes": 15,
                "cooldown_seconds": 1800,
                "max_triages_per_hour": 2,
                "env_vars": ["DD_API_KEY"]
            }
        }),
    );
}

#[test]
fn add_agent_firefighter_watch_defaults() {
    add_match("ff", json!({"type": "firefighter", "watch": {}}));
}

#[test]
fn add_agent_firefighter_no_watch_no_field() {
    // type=firefighter sem config.watch → não adiciona watch
    add_match("ff2", json!({"type": "firefighter"}));
}

#[test]
fn add_agent_watch_on_non_firefighter_ignored() {
    add_match(
        "s",
        json!({"type": "specialist", "watch": {"source": "x"}}),
    );
}

#[test]
fn add_agent_scope_gets_sanitized() {
    add_match(
        "s",
        json!({
            "type": "specialist",
            "scope": {"bash_allow": ["git *", "", 42, null, "ls"]}
        }),
    );
}

#[test]
fn add_agent_scope_non_object_reset() {
    add_match(
        "s",
        json!({"type": "specialist", "scope": "invalid"}),
    );
}

#[test]
fn add_agent_empty_tools_preserved() {
    add_match(
        "s",
        json!({"type": "specialist", "tools": []}),
    );
}

#[test]
fn add_agent_custom_role_only() {
    add_match(
        "s",
        json!({"type": "specialist", "role": "Cuida da qualidade"}),
    );
}

#[test]
fn add_agent_preserves_existing_context_file() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);

    fs::create_dir_all(a.join("agents")).unwrap();
    fs::create_dir_all(b.join("agents")).unwrap();
    fs::write(a.join("agents/foo.md"), "preexistente").unwrap();
    fs::write(b.join("agents/foo.md"), "preexistente").unwrap();

    let cfg = json!({"type": "specialist", "context": "novo"});
    let payload = json!({"name": "foo", "config": cfg});
    let v1 = call_v1("addAgent", &a, &payload).expect("v1 ok");
    let v2 = add_agent(&b, "foo", &cfg).expect("v2 ok");

    assert_eq!(v1, v2.to_string());
    assert_agents_json_match(&a, &b);
    assert_ctx_match(&a, &b, "agents/foo.md");
    // ambos devem ter mantido "preexistente"
    let md = fs::read_to_string(b.join("agents/foo.md")).unwrap();
    assert_eq!(md, "preexistente");
}

// ── add_agent errors ───────────────────────────────────────────────────────

#[test]
fn add_agent_invalid_name_both_error() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let payload = json!({"name": "Foo", "config": {"type": "specialist"}});
    let v1 = call_v1("addAgent", &a, &payload).expect_err("v1 deve lançar");
    assert!(v1.contains("Nome inválido"), "stderr={v1}");
    let v2 = add_agent(&b, "Foo", &json!({"type": "specialist"})).expect_err("v2 deve lançar");
    assert!(matches!(v2, domain_agents::AddAgentError::InvalidName));
}

#[test]
fn add_agent_invalid_type_both_error() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let payload = json!({"name": "foo", "config": {"type": "xyz"}});
    let v1 = call_v1("addAgent", &a, &payload).expect_err("v1 deve lançar");
    assert!(v1.contains("Tipo inválido"), "stderr={v1}");
    let v2 = add_agent(&b, "foo", &json!({"type": "xyz"})).expect_err("v2 deve lançar");
    assert!(matches!(
        v2,
        domain_agents::AddAgentError::InvalidType { .. }
    ));
}

#[test]
fn add_agent_duplicate_both_error() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);

    call_v1("addAgent", &a, &json!({"name":"foo","config":{"type":"specialist"}})).unwrap();
    add_agent(&b, "foo", &json!({"type": "specialist"})).unwrap();

    let v1_err = call_v1(
        "addAgent",
        &a,
        &json!({"name":"foo","config":{"type":"specialist"}}),
    )
    .expect_err("v1 deve lançar");
    assert!(v1_err.contains("já existe"), "stderr={v1_err}");
    let v2_err = add_agent(&b, "foo", &json!({"type": "specialist"})).expect_err("v2");
    assert!(matches!(
        v2_err,
        domain_agents::AddAgentError::AlreadyExists { .. }
    ));
}

// ── remove_agent ───────────────────────────────────────────────────────────

#[test]
fn remove_agent_basic() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);

    let add_payload = json!({"name":"foo","config":{"type":"specialist","context":"body"}});
    call_v1("addAgent", &a, &add_payload).unwrap();
    add_agent(&b, "foo", &json!({"type":"specialist","context":"body"})).unwrap();

    let rm_payload = json!({"name": "foo"});
    let v1 = call_v1("removeAgent", &a, &rm_payload).expect("v1 ok");
    let v2 = remove_agent(&b, "foo").expect("v2 ok");
    assert_eq!(v1, v2.to_string());
    assert_agents_json_match(&a, &b);
    assert!(!a.join("agents/foo.md").exists());
    assert!(!b.join("agents/foo.md").exists());
}

#[test]
fn remove_agent_missing_returns_false() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    call_v1(
        "addAgent",
        &a,
        &json!({"name":"other","config":{"type":"specialist"}}),
    )
    .unwrap();
    add_agent(&b, "other", &json!({"type":"specialist"})).unwrap();

    let rm_payload = json!({"name": "missing"});
    let v1 = call_v1("removeAgent", &a, &rm_payload).expect("v1 ok");
    let v2 = remove_agent(&b, "missing").expect("v2 ok");
    assert_eq!(v1, "false");
    assert_eq!(v1, v2.to_string());
    // manifest intocado
    assert_agents_json_match(&a, &b);
}

#[test]
fn remove_agent_no_manifest_returns_false() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let rm_payload = json!({"name": "foo"});
    let v1 = call_v1("removeAgent", &a, &rm_payload).expect("v1 ok");
    let v2 = remove_agent(&b, "foo").expect("v2 ok");
    assert_eq!(v1, "false");
    assert_eq!(v1, v2.to_string());
}

#[test]
fn remove_agent_custom_context_file() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);

    let cfg = json!({"type":"specialist","context_file":"custom/foo.md","context":"body"});
    call_v1("addAgent", &a, &json!({"name":"foo","config":cfg.clone()})).unwrap();
    add_agent(&b, "foo", &cfg).unwrap();

    assert!(a.join("custom/foo.md").exists());
    assert!(b.join("custom/foo.md").exists());

    remove_agent(&b, "foo").unwrap();
    call_v1("removeAgent", &a, &json!({"name":"foo"})).unwrap();

    assert!(!a.join("custom/foo.md").exists());
    assert!(!b.join("custom/foo.md").exists());
    assert_agents_json_match(&a, &b);
}
