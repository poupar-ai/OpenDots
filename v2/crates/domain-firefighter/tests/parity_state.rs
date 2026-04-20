//! Paridade v1 vs v2 para `loadState`, `saveState`, `pruneOldTriages`.
//! Compara byte-a-byte o arquivo `firefighter_state.json` gerado.

use domain_firefighter::{load_state, prune_old_triages, save_state, FixedClock};
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::TempDir;

const FIXED_MS: i64 = 1_776_600_000_000; // 2026-04-19T12:00:00.000Z

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

const lib = require(cjsPath);
let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', d => { input += d; });
process.stdin.on('end', () => {
  try {
    const { op, memDir, args } = JSON.parse(input);
    let out;
    if (op === 'saveState') {
      lib.saveState(memDir, args.state);
      out = null;
    } else if (op === 'loadState') {
      out = lib.loadState(memDir);
    } else if (op === 'pruneOldTriages') {
      // pruneOldTriages não é exportado, chamamos via saveState+checkAgent?
      // Em vez disso, reimplementamos a lógica aqui (copy-paste do .cjs):
      const cutoff = Date.now() - 3600_000;
      out = (args.entries || []).filter(e => new Date(e.ts).getTime() > cutoff);
    } else throw new Error('unknown op ' + op);
    process.stdout.write(JSON.stringify(out === undefined ? null : out));
  } catch (e) {
    process.stderr.write('THROW: ' + e.message);
    process.exit(3);
  }
});
"#;

fn cjs_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../plugin/scripts/firefighter.cjs")
        .canonicalize()
        .expect("firefighter.cjs missing")
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
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn save_state_byte_equal() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let state = json!({
        "manager": {
            "last_triage_at": "2026-04-19T11:50:00.000Z",
            "triages_last_hour": [
                {"ts": "2026-04-19T11:50:00.000Z", "events": 7}
            ]
        }
    });
    run_node(a.path(), "saveState", json!({"state": state}));
    save_state(b.path(), &state).unwrap();
    assert_eq!(
        fs::read_to_string(a.path().join("firefighter_state.json")).unwrap(),
        fs::read_to_string(b.path().join("firefighter_state.json")).unwrap()
    );
}

#[test]
fn save_state_empty_object() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let state = json!({});
    run_node(a.path(), "saveState", json!({"state": state}));
    save_state(b.path(), &state).unwrap();
    assert_eq!(
        fs::read_to_string(a.path().join("firefighter_state.json")).unwrap(),
        fs::read_to_string(b.path().join("firefighter_state.json")).unwrap()
    );
}

#[test]
fn load_state_missing_file() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let v1 = run_node(a.path(), "loadState", json!({}));
    let v2 = load_state(b.path());
    assert_eq!(v1, v2);
    assert_eq!(v1, json!({}));
}

#[test]
fn load_state_malformed_json_returns_empty() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    fs::write(a.path().join("firefighter_state.json"), "not json {").unwrap();
    fs::write(b.path().join("firefighter_state.json"), "not json {").unwrap();
    let v1 = run_node(a.path(), "loadState", json!({}));
    let v2 = load_state(b.path());
    assert_eq!(v1, v2);
    assert_eq!(v1, json!({}));
}

#[test]
fn prune_old_triages_parity_basic() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let tmp = TempDir::new().unwrap();
    // now = FIXED_MS. cutoff = FIXED_MS - 3_600_000.
    // 30min atrás → mantém. 2h atrás → descarta.
    let entries = json!([
        {"ts": "2026-04-19T11:30:00.000Z", "events": 1},
        {"ts": "2026-04-19T10:00:00.000Z", "events": 2},
        {"ts": "2026-04-19T11:59:00.000Z", "events": 3},
    ]);
    let v1 = run_node(tmp.path(), "pruneOldTriages", json!({"entries": entries}));
    let v2 = prune_old_triages(entries.as_array().unwrap(), &FixedClock(FIXED_MS));
    assert_eq!(v1, Value::Array(v2));
}

#[test]
fn prune_old_triages_exact_cutoff_excluded() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let tmp = TempDir::new().unwrap();
    // exatamente 1h atrás → filtro é `>` strict, então sai fora.
    let entries = json!([
        {"ts": "2026-04-19T11:00:00.000Z", "events": 1},
        {"ts": "2026-04-19T11:00:00.001Z", "events": 2},
    ]);
    let v1 = run_node(tmp.path(), "pruneOldTriages", json!({"entries": entries}));
    let v2 = prune_old_triages(entries.as_array().unwrap(), &FixedClock(FIXED_MS));
    assert_eq!(v1, Value::Array(v2));
}

#[test]
fn prune_old_triages_invalid_ts_is_dropped() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let tmp = TempDir::new().unwrap();
    // JS: `new Date("bogus").getTime()` → NaN, NaN > cutoff → false → dropped.
    let entries = json!([
        {"ts": "bogus", "events": 1},
        {"ts": "2026-04-19T11:55:00.000Z", "events": 2},
    ]);
    let v1 = run_node(tmp.path(), "pruneOldTriages", json!({"entries": entries}));
    let v2 = prune_old_triages(entries.as_array().unwrap(), &FixedClock(FIXED_MS));
    assert_eq!(v1, Value::Array(v2));
}
