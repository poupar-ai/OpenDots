//! Paridade v1 vs v2 para funções puras de derivação de estado:
//! `deriveState`, `traceSummary`, `getMessage`, `getPending`, `getState`.

use domain_bus::{derive_state_ordered, get_pending, get_state, trace_summary, FixedClock, GetStateOpts};
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::TempDir;

const FIXED_MS: i64 = 1_776_600_000_000; // 2026-04-19T12:00:00Z

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
    if (op === 'deriveState') {
      // Map → Array de [k, v]
      const m = lib.deriveState(args.events);
      out = Array.from(m.entries());
    } else if (op === 'traceSummary') {
      out = lib.traceSummary(args.events, args.trace_id);
    } else if (op === 'getPending') {
      out = lib.getPending(memDir);
    } else if (op === 'getState') {
      out = lib.getState(memDir, args.opts || {});
    } else {
      throw new Error('unknown op ' + op);
    }
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
    let input = json!({"op": op, "memDir": mem_dir.to_string_lossy(), "args": args});
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

fn seed_chunks(dir: &Path, files: &[(&str, &str)]) {
    fs::create_dir_all(dir.join("messages")).unwrap();
    for (day, content) in files {
        fs::write(dir.join(format!("messages/{day}.jsonl")), content).unwrap();
    }
}

#[test]
fn derive_state_matches_cjs_map_order() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let events = json!([
        {"ts":"2026-04-19T00:00:00.000Z","type":"created","trace_id":"t1"},
        {"ts":"2026-04-19T00:00:01.000Z","type":"created","trace_id":"t2"},
        {"ts":"2026-04-19T00:00:02.000Z","type":"delivered","trace_id":"t1"},
        {"ts":"2026-04-19T00:00:03.000Z","type":"responded","trace_id":"t1"},
        {"ts":"2026-04-19T00:00:04.000Z","type":"delivered","trace_id":"t2"},
    ]);
    let v1 = run_node(
        a.path(),
        "deriveState",
        json!({"events": events.clone()}),
    );
    let events_vec: Vec<Value> = events.as_array().unwrap().clone();
    let v2_map = derive_state_ordered(&events_vec);
    let v2: Value = Value::Array(
        v2_map
            .into_iter()
            .map(|(k, v)| json!([k, v]))
            .collect(),
    );
    assert_eq!(v1, v2);
}

#[test]
fn trace_summary_matches_cjs() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let events = json!([
        {"ts":"2026-04-19T00:00:00.000Z","type":"created","trace_id":"t1","from":"a"},
        {"ts":"2026-04-19T00:00:01.000Z","type":"retry","trace_id":"t1","attempt":1},
        {"ts":"2026-04-19T00:00:02.000Z","type":"delivered","trace_id":"t1"},
        {"ts":"2026-04-19T00:00:03.000Z","type":"responded","trace_id":"t1"},
    ]);
    let v1 = run_node(
        a.path(),
        "traceSummary",
        json!({"events": events.clone(), "trace_id": "t1"}),
    );
    let events_vec: Vec<Value> = events.as_array().unwrap().clone();
    let s = trace_summary(&events_vec, "t1").unwrap();
    let v2 = json!({
        "created": s.created,
        "latest": s.latest,
        "attempts": s.attempts,
        "events": s.events,
    });
    assert_eq!(v1, v2);
}

#[test]
fn trace_summary_absent_returns_null() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let v1 = run_node(
        a.path(),
        "traceSummary",
        json!({"events": [], "trace_id": "none"}),
    );
    assert_eq!(v1, Value::Null);
    assert!(trace_summary(&[], "none").is_none());
}

#[test]
fn get_pending_matches_cjs() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let lines = r#"{"ts":"2026-04-19T00:00:00.000Z","type":"created","trace_id":"p1","from":"a"}
{"ts":"2026-04-19T00:00:01.000Z","type":"delivered","trace_id":"p1"}
{"ts":"2026-04-19T00:00:02.000Z","type":"created","trace_id":"p2","from":"b"}
{"ts":"2026-04-19T00:00:03.000Z","type":"created","trace_id":"p3","from":"c"}
{"ts":"2026-04-19T00:00:04.000Z","type":"delivered","trace_id":"p3"}
{"ts":"2026-04-19T00:00:05.000Z","type":"responded","trace_id":"p3"}
"#;
    seed_chunks(a.path(), &[("2026-04-19", lines)]);
    seed_chunks(b.path(), &[("2026-04-19", lines)]);

    let v1 = run_node(a.path(), "getPending", json!({}));
    let c = FixedClock(FIXED_MS);
    let v2 = Value::Array(get_pending(b.path(), &c));
    assert_eq!(v1, v2);
}

#[test]
fn get_state_matches_cjs() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let lines = r#"{"ts":"2026-04-18T00:00:00.000Z","type":"created","trace_id":"x1"}
{"ts":"2026-04-19T00:00:00.000Z","type":"created","trace_id":"x2"}
{"ts":"2026-04-19T00:00:01.000Z","type":"delivered","trace_id":"x2"}
{"ts":"2026-04-19T00:00:02.000Z","type":"responded","trace_id":"x2"}
{"ts":"2026-04-19T00:00:03.000Z","type":"created","trace_id":"x3"}
{"ts":"2026-04-19T00:00:04.000Z","type":"failed","trace_id":"x3","reason":"manual"}
"#;
    // Divide em 2 dias pra exercitar list_day_files sort.
    let day_18 = r#"{"ts":"2026-04-18T00:00:00.000Z","type":"created","trace_id":"x1"}
"#;
    let day_19 = r#"{"ts":"2026-04-19T00:00:00.000Z","type":"created","trace_id":"x2"}
{"ts":"2026-04-19T00:00:01.000Z","type":"delivered","trace_id":"x2"}
{"ts":"2026-04-19T00:00:02.000Z","type":"responded","trace_id":"x2"}
{"ts":"2026-04-19T00:00:03.000Z","type":"created","trace_id":"x3"}
{"ts":"2026-04-19T00:00:04.000Z","type":"failed","trace_id":"x3","reason":"manual"}
"#;
    let _ = lines;
    seed_chunks(
        a.path(),
        &[("2026-04-18", day_18), ("2026-04-19", day_19)],
    );
    seed_chunks(
        b.path(),
        &[("2026-04-18", day_18), ("2026-04-19", day_19)],
    );

    let v1 = run_node(a.path(), "getState", json!({"opts": {"recent": 50}}));
    let c = FixedClock(FIXED_MS);
    let v2 = get_state(
        b.path(),
        &GetStateOpts {
            recent: 50,
            since_days: None,
        },
        &c,
    );
    assert_eq!(v1, v2);
}

#[test]
fn get_state_recent_limit() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let mut lines = String::new();
    for i in 0..10 {
        lines.push_str(&format!(
            "{{\"ts\":\"2026-04-19T00:00:{:02}.000Z\",\"type\":\"created\",\"trace_id\":\"g{i}\"}}\n",
            i
        ));
    }
    seed_chunks(a.path(), &[("2026-04-19", &lines)]);
    seed_chunks(b.path(), &[("2026-04-19", &lines)]);

    let v1 = run_node(a.path(), "getState", json!({"opts": {"recent": 3}}));
    let c = FixedClock(FIXED_MS);
    let v2 = get_state(
        b.path(),
        &GetStateOpts {
            recent: 3,
            since_days: None,
        },
        &c,
    );
    assert_eq!(v1, v2);
}
