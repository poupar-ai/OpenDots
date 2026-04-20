//! Paridade v1 vs v2 para `sweepPending` — retry + timeout + failed.

use domain_bus::{sweep_pending, FixedClock, SweepCallbacks, SweepOpts};
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::TempDir;

const FIXED_MS: i64 = 1_776_600_000_000; // 2026-04-19T12:00:00Z

/// Importante: precisamos shimar `Date` *e* `Date.now()` com um valor
/// consistente — o sweep usa `Date.now()` diretamente e `new Date(ts)`
/// para parsear os timestamps dos eventos.
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
    const { memDir, args } = JSON.parse(input);
    const retryCalls = [];
    const timeoutCalls = [];
    const out = lib.sweepPending(memDir, {
      now: args.now,
      onRetry:   (created) => retryCalls.push(created),
      onTimeout: (created) => timeoutCalls.push(created),
    });
    process.stdout.write(JSON.stringify({ result: out, retryCalls, timeoutCalls }));
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

fn run_node(mem_dir: &Path, args: Value) -> Value {
    let input = json!({"memDir": mem_dir.to_string_lossy(), "args": args});
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

fn seed(dir: &Path, day: &str, content: &str) {
    fs::create_dir_all(dir.join("messages")).unwrap();
    fs::write(dir.join(format!("messages/{day}.jsonl")), content).unwrap();
}

#[test]
fn sweep_emits_retry_after_delivery_deadline() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    // created faz 2 min atrás (> 90s deadline). Sem delivered. Uma só tentativa.
    let content = r#"{"ts":"2026-04-19T11:57:00.000Z","type":"created","trace_id":"r1","from":"x"}
"#;
    seed(a.path(), "2026-04-19", content);
    seed(b.path(), "2026-04-19", content);

    let v1 = run_node(a.path(), json!({}));
    let c = FixedClock(FIXED_MS);
    let mut cb = SweepCallbacks::default();
    let retry_calls: std::rc::Rc<std::cell::RefCell<Vec<Value>>> = Default::default();
    let rc_clone = retry_calls.clone();
    cb.on_retry = Some(Box::new(move |created| {
        rc_clone.borrow_mut().push(created.clone());
    }));
    let r2 = sweep_pending(b.path(), &SweepOpts::default(), &mut cb, &c);

    // Comparar effects on disk + results.
    assert_eq!(
        read_dir_contents(a.path()),
        read_dir_contents(b.path()),
        "jsonl diverge"
    );
    // Confirmar que v1 reporta 1 retry.
    let v1_retried = v1["result"]["retried"].as_array().unwrap();
    assert_eq!(v1_retried.len(), 1);
    assert_eq!(v1_retried[0]["outcome"], "retry");
    assert_eq!(v1_retried[0]["attempt"], 1);
    assert_eq!(r2.retried.len(), 1);
    // Callbacks disparados no v1 também?
    assert_eq!(v1["retryCalls"].as_array().unwrap().len(), 1);
    assert_eq!(retry_calls.borrow().len(), 1);
}

#[test]
fn sweep_emits_failed_after_max_attempts() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    // 3 retries acumulados, último evento é retry (stage=1). age > 90s.
    // attempts = count(delivered|retry) = 3 ≥ MAX_ATTEMPTS → failed.
    let content = r#"{"ts":"2026-04-19T11:57:00.000Z","type":"created","trace_id":"f1"}
{"ts":"2026-04-19T11:57:01.000Z","type":"retry","trace_id":"f1","attempt":1}
{"ts":"2026-04-19T11:57:02.000Z","type":"retry","trace_id":"f1","attempt":2}
{"ts":"2026-04-19T11:57:03.000Z","type":"retry","trace_id":"f1","attempt":3}
"#;
    seed(a.path(), "2026-04-19", content);
    seed(b.path(), "2026-04-19", content);

    let v1 = run_node(a.path(), json!({}));
    let c = FixedClock(FIXED_MS);
    let mut cb = SweepCallbacks::default();
    let r2 = sweep_pending(b.path(), &SweepOpts::default(), &mut cb, &c);

    let v1_retried = v1["result"]["retried"].as_array().unwrap();
    assert_eq!(v1_retried.len(), 1);
    assert_eq!(v1_retried[0]["outcome"], "failed");
    assert_eq!(r2.retried.len(), 1);
    assert!(matches!(
        r2.retried[0].1,
        domain_bus::SweepOutcome::Failed
    ));
    assert_eq!(read_dir_contents(a.path()), read_dir_contents(b.path()));
}

#[test]
fn sweep_emits_timed_out_after_response_deadline() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    // delivered faz 10 min (> 5min). Deve virar timed_out.
    let content = r#"{"ts":"2026-04-19T11:50:00.000Z","type":"created","trace_id":"t1"}
{"ts":"2026-04-19T11:50:05.000Z","type":"delivered","trace_id":"t1"}
"#;
    seed(a.path(), "2026-04-19", content);
    seed(b.path(), "2026-04-19", content);

    let v1 = run_node(a.path(), json!({}));
    let c = FixedClock(FIXED_MS);
    let mut cb = SweepCallbacks::default();
    let to_calls: std::rc::Rc<std::cell::RefCell<Vec<Value>>> = Default::default();
    let to_clone = to_calls.clone();
    cb.on_timeout = Some(Box::new(move |created| {
        to_clone.borrow_mut().push(created.clone());
    }));
    let r2 = sweep_pending(b.path(), &SweepOpts::default(), &mut cb, &c);

    assert_eq!(v1["result"]["timedOut"].as_array().unwrap().len(), 1);
    assert_eq!(r2.timed_out.len(), 1);
    assert_eq!(v1["timeoutCalls"].as_array().unwrap().len(), 1);
    assert_eq!(to_calls.borrow().len(), 1);
    assert_eq!(read_dir_contents(a.path()), read_dir_contents(b.path()));
}

#[test]
fn sweep_noop_when_no_pending() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    // Apenas trace terminal.
    let content = r#"{"ts":"2026-04-19T11:00:00.000Z","type":"created","trace_id":"ok"}
{"ts":"2026-04-19T11:00:01.000Z","type":"delivered","trace_id":"ok"}
{"ts":"2026-04-19T11:00:02.000Z","type":"responded","trace_id":"ok"}
"#;
    seed(a.path(), "2026-04-19", content);
    seed(b.path(), "2026-04-19", content);

    let v1 = run_node(a.path(), json!({}));
    let c = FixedClock(FIXED_MS);
    let mut cb = SweepCallbacks::default();
    let r2 = sweep_pending(b.path(), &SweepOpts::default(), &mut cb, &c);

    assert!(v1["result"]["retried"].as_array().unwrap().is_empty());
    assert!(v1["result"]["timedOut"].as_array().unwrap().is_empty());
    assert!(r2.retried.is_empty());
    assert!(r2.timed_out.is_empty());
    assert_eq!(read_dir_contents(a.path()), read_dir_contents(b.path()));
}

#[test]
fn sweep_ignores_young_created() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    // created 10s atrás — abaixo de DELIVERY_DEADLINE_MS (90s).
    let content = r#"{"ts":"2026-04-19T11:59:50.000Z","type":"created","trace_id":"young"}
"#;
    seed(a.path(), "2026-04-19", content);
    seed(b.path(), "2026-04-19", content);

    let v1 = run_node(a.path(), json!({}));
    let c = FixedClock(FIXED_MS);
    let mut cb = SweepCallbacks::default();
    let r2 = sweep_pending(b.path(), &SweepOpts::default(), &mut cb, &c);

    assert!(v1["result"]["retried"].as_array().unwrap().is_empty());
    assert!(r2.retried.is_empty());
    assert_eq!(read_dir_contents(a.path()), read_dir_contents(b.path()));
}
