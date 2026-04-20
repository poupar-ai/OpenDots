//! Paridade v1 (`bus.cjs::emit` + `readEvents`) vs v2.
//!
//! Desafio: `new Date().toISOString()` e `Date.now()` do .cjs são
//! não-determinísticos. Solução: monkey-patch de ambos via script Node
//! antes de require.

use domain_bus::{emit, read_events, FixedClock, ReadEventsOpts};
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::TempDir;

const FIXED_ISO: &str = "2026-04-19T12:00:00.000Z";
const FIXED_MS: i64 = 1_776_600_000_000;

/// Shim de `Date`: `new Date()` (sem args) retorna Date fixo; tudo
/// o mais passa-through. Patch de `Date.now()` também.
/// Preserva `Date.prototype` intacto pra suportar `setUTCDate`,
/// `toISOString` nativo, etc.
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
DateShim.now   = () => msFixed;
DateShim.UTC   = _Orig.UTC.bind(_Orig);
DateShim.parse = _Orig.parse.bind(_Orig);
global.Date = DateShim;

const lib = require(cjsPath);
let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', d => { input += d; });
process.stdin.on('end', () => {
  try {
    const { op, memDir, args } = JSON.parse(input);
    let out;
    if (op === 'emit') out = lib.emit(memDir, args.ev);
    else if (op === 'readEvents') out = lib.readEvents(memDir, args.opts || {});
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
        .join("../../../plugin/scripts/bus.cjs")
        .canonicalize()
        .expect("bus.cjs missing")
}

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_node(mem_dir: &Path, op: &str, args: Value) -> Value {
    let input = json!({
        "op": op,
        "memDir": mem_dir.to_string_lossy(),
        "args": args,
    });
    let mut child = Command::new("node")
        .arg("-e")
        .arg(NODE_SCRIPT)
        .arg(cjs_path())
        .arg(FIXED_ISO)
        .arg(FIXED_MS.to_string())
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

fn read_dir_contents(dir: &Path) -> Vec<(String, String)> {
    let mut v = Vec::new();
    if let Ok(entries) = fs::read_dir(dir.join("messages")) {
        for e in entries.flatten() {
            let name = e.file_name().into_string().unwrap_or_default();
            let c = fs::read_to_string(e.path()).unwrap_or_default();
            v.push((name, c));
        }
    }
    v.sort();
    v
}

#[test]
fn emit_produces_same_jsonl() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let ev = json!({"type":"created","trace_id":"t1","from":"x","to":"y"});

    let v1 = run_node(a.path(), "emit", json!({"ev": ev}));
    let c = FixedClock(FIXED_MS);
    let v2 = emit(b.path(), &ev, &c).unwrap();

    assert_eq!(v1, v2, "return value diverge");
    assert_eq!(read_dir_contents(a.path()), read_dir_contents(b.path()));
}

#[test]
fn emit_preserves_existing_ts() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let ev = json!({"type":"retry","trace_id":"t2","ts":"2026-03-15T10:20:30.500Z","foo":1});

    let v1 = run_node(a.path(), "emit", json!({"ev": ev}));
    let c = FixedClock(FIXED_MS);
    let v2 = emit(b.path(), &ev, &c).unwrap();

    assert_eq!(v1, v2);
    // Como o ts fornecido é de 2026-03-15, o chunk deve ser desse dia.
    assert!(a.path().join("messages/2026-03-15.jsonl").exists());
    assert!(b.path().join("messages/2026-03-15.jsonl").exists());
    assert_eq!(read_dir_contents(a.path()), read_dir_contents(b.path()));
}

#[test]
fn emit_invalid_returns_undefined() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    // Sem type/trace_id → ambos devem retornar undefined/None.
    let v1 = run_node(a.path(), "emit", json!({"ev": {"foo":"bar"}}));
    assert_eq!(v1, Value::Null);
    let c = FixedClock(FIXED_MS);
    assert!(emit(b.path(), &json!({"foo":"bar"}), &c).is_none());
    // Nenhum arquivo deve ter sido criado.
    assert!(read_dir_contents(a.path()).is_empty());
    assert!(read_dir_contents(b.path()).is_empty());
}

#[test]
fn emit_sequence_appends_to_same_chunk() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let c = FixedClock(FIXED_MS);
    for i in 1..=3 {
        let ev = json!({"type":"created","trace_id": format!("t{i}"), "i": i});
        run_node(a.path(), "emit", json!({"ev": ev}));
        emit(b.path(), &ev, &c).unwrap();
    }
    assert_eq!(read_dir_contents(a.path()), read_dir_contents(b.path()));
}

#[test]
fn read_events_filters_by_since_date() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    // Escrever chunks em 3 dias, bypass do emit pra fixar ts arbitrário.
    fs::create_dir_all(a.path().join("messages")).unwrap();
    fs::create_dir_all(b.path().join("messages")).unwrap();
    for day in ["2026-04-17", "2026-04-18", "2026-04-19"] {
        let line = format!(
            "{{\"ts\":\"{day}T00:00:00.000Z\",\"type\":\"created\",\"trace_id\":\"t{day}\"}}\n"
        );
        fs::write(a.path().join(format!("messages/{day}.jsonl")), &line).unwrap();
        fs::write(b.path().join(format!("messages/{day}.jsonl")), &line).unwrap();
    }

    let v1 = run_node(
        a.path(),
        "readEvents",
        json!({"opts": {"sinceDate": "2026-04-18"}}),
    );
    let c = FixedClock(FIXED_MS);
    let v2 = read_events(
        b.path(),
        &ReadEventsOpts {
            since_days: None,
            since_date: Some("2026-04-18"),
        },
        &c,
    );
    let v2v = Value::Array(v2);
    assert_eq!(v1, v2v);
}

#[test]
fn read_events_filters_by_since_days() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    fs::create_dir_all(a.path().join("messages")).unwrap();
    fs::create_dir_all(b.path().join("messages")).unwrap();
    for day in ["2026-04-10", "2026-04-17", "2026-04-18", "2026-04-19"] {
        let line = format!(
            "{{\"ts\":\"{day}T00:00:00.000Z\",\"type\":\"created\",\"trace_id\":\"t{day}\"}}\n"
        );
        fs::write(a.path().join(format!("messages/{day}.jsonl")), &line).unwrap();
        fs::write(b.path().join(format!("messages/{day}.jsonl")), &line).unwrap();
    }

    // sinceDays=3 no dia 2026-04-19 → cutoff 2026-04-17. Dia 10 fora.
    let v1 = run_node(a.path(), "readEvents", json!({"opts": {"sinceDays": 3}}));
    let c = FixedClock(FIXED_MS);
    let v2 = read_events(
        b.path(),
        &ReadEventsOpts {
            since_days: Some(3),
            since_date: None,
        },
        &c,
    );
    let v2v = Value::Array(v2);
    assert_eq!(v1, v2v);
}
