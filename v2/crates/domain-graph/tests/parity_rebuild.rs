//! Paridade v1 vs v2 para `rebuildGraph`. Semeia JSONLs em dois tmp
//! dirs e compara `graph.json` byte-a-byte.

use domain_graph::{rebuild_graph, FixedClock};
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::TempDir;

const FIXED_ISO: &str = "2026-04-19T12:00:00.000Z";
const FIXED_MS: i64 = 1_776_600_000_000;

const NODE_SCRIPT: &str = r#"
const cjsPath = process.argv[1];
const msFixed = Number(process.argv[3]);
const _Orig = Date;
function DateShim(...args) {
  if (!new.target) return _Orig();
  if (args.length === 0) return new _Orig(msFixed);
  return new _Orig(...args);
}
DateShim.prototype = _Orig.prototype;
DateShim.now = () => msFixed;
DateShim.UTC = _Orig.UTC.bind(_Orig);
DateShim.parse = _Orig.parse.bind(_Orig);
global.Date = DateShim;

const { rebuildGraph } = require(cjsPath);
const { readJsonl } = require(require('path').join(require('path').dirname(cjsPath), 'lib.cjs'));

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', d => { input += d; });
process.stdin.on('end', () => {
  try {
    const { memDir } = JSON.parse(input);
    rebuildGraph(memDir, readJsonl);
    process.stdout.write('OK');
  } catch (e) {
    process.stderr.write('THROW: ' + e.message + '\n' + e.stack);
    process.exit(3);
  }
});
"#;

fn cjs_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../plugin/scripts/graph.cjs")
        .canonicalize()
        .expect("graph.cjs missing")
}

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_node(mem_dir: &Path) {
    let input = json!({"memDir": mem_dir.to_string_lossy()});
    let mut child = Command::new("node")
        .arg("-e")
        .arg(NODE_SCRIPT)
        .arg(cjs_path())
        .arg("unused")
        .arg(FIXED_MS.to_string())
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
    if !out.status.success() {
        panic!("node exited: {}", String::from_utf8_lossy(&out.stderr));
    }
}

fn seed_jsonl(dir: &Path, name: &str, rows: &[Value]) {
    let mut s = String::new();
    for r in rows {
        s.push_str(&r.to_string());
        s.push('\n');
    }
    fs::write(dir.join(name), s).unwrap();
}

fn seed_both(a: &Path, b: &Path, name: &str, rows: &[Value]) {
    seed_jsonl(a, name, rows);
    seed_jsonl(b, name, rows);
}

/// Normaliza `updated_at`/`rebuilt_at` para comparar só estrutura.
fn norm(v: &mut Value) {
    if let Some(o) = v.as_object_mut() {
        if o.contains_key("rebuilt_at") {
            o["rebuilt_at"] = Value::String("_".into());
        }
        if o.contains_key("updated_at") {
            o["updated_at"] = Value::String("_".into());
        }
    }
}

fn read_graph(dir: &Path) -> Value {
    let raw = fs::read_to_string(dir.join("graph.json")).unwrap();
    serde_json::from_str(&raw).unwrap()
}

#[test]
fn rebuild_empty_dirs_equal() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    run_node(a.path());
    rebuild_graph(b.path(), &FixedClock(FIXED_MS)).unwrap();
    // Com dirs vazios, os .jsonl são inexistentes → graph vazio.
    // Paridade total pois rebuilt_at/updated_at vêm do FIXED_MS (idêntico).
    assert_eq!(
        fs::read_to_string(a.path().join("graph.json")).unwrap(),
        fs::read_to_string(b.path().join("graph.json")).unwrap()
    );
}

#[test]
fn rebuild_observations_co_edit_byte_equal() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let obs = vec![
        json!({"type":"file_edit","file":"src/a.rs","ts":"2026-04-19T11:50:00.000Z","session_id":"s1"}),
        json!({"type":"file_edit","file":"src/b.rs","ts":"2026-04-19T11:51:00.000Z","session_id":"s1"}),
        json!({"type":"bash","cmd":"cargo test --lib","ts":"2026-04-19T11:52:00.000Z","session_id":"s1"}),
        json!({"type":"file_edit","file":"src/a.rs","ts":"2026-04-19T11:53:00.000Z","session_id":"s1"}),
    ];
    seed_both(a.path(), b.path(), "observations.jsonl", &obs);
    run_node(a.path());
    rebuild_graph(b.path(), &FixedClock(FIXED_MS)).unwrap();
    assert_eq!(
        fs::read_to_string(a.path().join("graph.json")).unwrap(),
        fs::read_to_string(b.path().join("graph.json")).unwrap()
    );
}

#[test]
fn rebuild_with_agent_logs_byte_equal() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    fs::create_dir_all(a.path().join("agent_logs")).unwrap();
    fs::create_dir_all(b.path().join("agent_logs")).unwrap();
    let rows = vec![
        json!({"type":"handoff_out","target":"worker","intent":"ask","ts":"2026-04-19T11:50:00.000Z"}),
        json!({"type":"handoff_out","target":"worker","intent":"ask","ts":"2026-04-19T11:51:00.000Z"}),
        json!({"type":"violation","target":"worker","ts":"2026-04-19T11:52:00.000Z"}),
    ];
    seed_both(
        &a.path().join("agent_logs"),
        &b.path().join("agent_logs"),
        "manager.jsonl",
        &rows,
    );
    run_node(a.path());
    rebuild_graph(b.path(), &FixedClock(FIXED_MS)).unwrap();
    assert_eq!(
        fs::read_to_string(a.path().join("graph.json")).unwrap(),
        fs::read_to_string(b.path().join("graph.json")).unwrap()
    );
}

#[test]
fn rebuild_with_decisions_and_gotchas() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let obs = vec![
        json!({"type":"file_edit","file":"src/x.rs","ts":"2026-04-19T11:50:00.000Z","session_id":"sess-1"}),
        json!({"type":"file_edit","file":"src/y.rs","ts":"2026-04-19T11:51:00.000Z","session_id":"sess-1"}),
    ];
    seed_both(a.path(), b.path(), "observations.jsonl", &obs);
    let decisions = vec![
        json!({"what":"Adotar regex unified","ts":"2026-04-19T11:55:00.000Z","session_id":"sess-1"}),
    ];
    seed_both(a.path(), b.path(), "decisions.jsonl", &decisions);
    let gotchas = vec![
        json!({"file":"src/x.rs","description":"UTF-16 off-by-one","ts":"2026-04-19T11:56:00.000Z"}),
    ];
    seed_both(a.path(), b.path(), "gotchas.jsonl", &gotchas);

    run_node(a.path());
    rebuild_graph(b.path(), &FixedClock(FIXED_MS)).unwrap();

    // Full byte-exact file comparison.
    let raw_a = fs::read_to_string(a.path().join("graph.json")).unwrap();
    let raw_b = fs::read_to_string(b.path().join("graph.json")).unwrap();
    assert_eq!(raw_a, raw_b);
}

#[test]
fn rebuild_with_humans_and_agents() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();

    // humans.json + agents.json
    let humans = json!({"humans":{"tiago":{"name":"Tiago"}}});
    let agents = json!({"agents":{"manager":{},"worker":{}}});
    fs::write(a.path().join("humans.json"), humans.to_string()).unwrap();
    fs::write(b.path().join("humans.json"), humans.to_string()).unwrap();
    fs::write(a.path().join("agents.json"), agents.to_string()).unwrap();
    fs::write(b.path().join("agents.json"), agents.to_string()).unwrap();

    // agent_chats — human falando para manager, agente falando para worker.
    fs::create_dir_all(a.path().join("agent_chats")).unwrap();
    fs::create_dir_all(b.path().join("agent_chats")).unwrap();
    let chat_manager = vec![
        json!({"ts":"2026-04-19T11:50:00.000Z","role":"user","from":"tiago","content":"hi"}),
        json!({"ts":"2026-04-19T11:51:00.000Z","role":"user","from":"worker","content":"yo"}),
    ];
    seed_both(
        &a.path().join("agent_chats"),
        &b.path().join("agent_chats"),
        "manager.jsonl",
        &chat_manager,
    );

    // human_chats
    fs::create_dir_all(a.path().join("human_chats")).unwrap();
    fs::create_dir_all(b.path().join("human_chats")).unwrap();
    let hc = vec![
        json!({"ts":"2026-04-19T11:53:00.000Z","from":"tiago","to":"maria","content":"oi"}),
    ];
    seed_both(
        &a.path().join("human_chats"),
        &b.path().join("human_chats"),
        "tiago__maria.jsonl",
        &hc,
    );

    run_node(a.path());
    rebuild_graph(b.path(), &FixedClock(FIXED_MS)).unwrap();

    assert_eq!(
        fs::read_to_string(a.path().join("graph.json")).unwrap(),
        fs::read_to_string(b.path().join("graph.json")).unwrap()
    );
}

#[test]
fn rebuild_norm_updated_at_timestamps_match() {
    // Sanity — quando não shimamos Date, verifica só estrutura.
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let obs = vec![
        json!({"type":"file_edit","file":"z.rs","ts":"2026-04-19T11:50:00.000Z","session_id":"s"}),
    ];
    seed_both(a.path(), b.path(), "observations.jsonl", &obs);
    run_node(a.path());
    rebuild_graph(b.path(), &FixedClock(FIXED_MS)).unwrap();

    let mut va = read_graph(a.path());
    let mut vb = read_graph(b.path());
    norm(&mut va);
    norm(&mut vb);
    assert_eq!(va, vb);
    // Garantia extra: ISO from FIXED_MS em ambos.
    assert_eq!(read_graph(a.path())["rebuilt_at"], FIXED_ISO);
    assert_eq!(read_graph(b.path())["rebuilt_at"], FIXED_ISO);
}

#[test]
fn rebuild_agent_call_creates_invokes() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let obs = vec![
        json!({"type":"agent_call","agent":"worker","agent_id":"manager","ts":"2026-04-19T11:50:00.000Z"}),
    ];
    seed_both(a.path(), b.path(), "observations.jsonl", &obs);
    run_node(a.path());
    rebuild_graph(b.path(), &FixedClock(FIXED_MS)).unwrap();
    assert_eq!(
        fs::read_to_string(a.path().join("graph.json")).unwrap(),
        fs::read_to_string(b.path().join("graph.json")).unwrap()
    );
}
