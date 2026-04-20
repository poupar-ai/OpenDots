//! Paridade v1 vs v2 para `migrateLegacy`, `compactOldFiles`,
//! `getConfig`/`setConfig` e `listDayFiles`.

use domain_bus::{
    compact_old_files, get_config, list_day_files, migrate_legacy, set_config, CompactOpts,
    FixedClock,
};
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
    if (op === 'migrateLegacy') out = lib.migrateLegacy(memDir);
    else if (op === 'compactOldFiles') out = lib.compactOldFiles(memDir, args.opts || {});
    else if (op === 'getConfig') out = lib.getConfig(memDir);
    else if (op === 'setConfig') {
      try { out = lib.setConfig(memDir, args.patch); }
      catch (e) { out = { __error: e.message }; }
    }
    else if (op === 'listDayFiles') out = lib.listDayFiles(memDir);
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

fn read_messages(dir: &Path) -> Vec<(String, String)> {
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
fn migrate_legacy_splits_by_day() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let legacy = "\
{\"ts\":\"2026-04-17T00:00:00.000Z\",\"type\":\"created\",\"trace_id\":\"m1\"}
{\"ts\":\"2026-04-18T00:00:00.000Z\",\"type\":\"created\",\"trace_id\":\"m2\"}
{\"ts\":\"2026-04-17T00:00:01.000Z\",\"type\":\"delivered\",\"trace_id\":\"m1\"}
invalid json
{\"ts\":\"2026-04-19T00:00:00.000Z\",\"type\":\"responded\",\"trace_id\":\"m1\"}
";
    fs::write(a.path().join("messages.jsonl"), legacy).unwrap();
    fs::write(b.path().join("messages.jsonl"), legacy).unwrap();

    let v1 = run_node(a.path(), "migrateLegacy", json!({}));
    let c = FixedClock(FIXED_MS);
    let r2 = migrate_legacy(b.path(), &c);

    let v2 = json!({ "migrated": r2.migrated, "days": r2.days });
    assert_eq!(v1, v2);
    assert_eq!(read_messages(a.path()), read_messages(b.path()));
    assert!(a.path().join("messages.jsonl.migrated").exists());
    assert!(b.path().join("messages.jsonl.migrated").exists());
    assert!(!a.path().join("messages.jsonl").exists());
    assert!(!b.path().join("messages.jsonl").exists());
}

#[test]
fn migrate_legacy_noop_when_absent() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let v1 = run_node(a.path(), "migrateLegacy", json!({}));
    let c = FixedClock(FIXED_MS);
    let r2 = migrate_legacy(b.path(), &c);
    // v1 = { migrated: 0 }; v2 struct {migrated: 0, days: 0}
    // Parser no .cjs retorna {migrated:0} sem days. Paridade: checar so migrated=0.
    assert_eq!(v1["migrated"], 0);
    assert_eq!(r2.migrated, 0);
}

#[test]
fn compact_old_files_deletes_same_set() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    for dir in [a.path(), b.path()] {
        fs::create_dir_all(dir.join("messages")).unwrap();
        for day in [
            "2026-03-01",
            "2026-04-01",
            "2026-04-05",
            "2026-04-10",
            "2026-04-19",
        ] {
            let line = format!(
                "{{\"ts\":\"{day}T00:00:00.000Z\",\"type\":\"created\",\"trace_id\":\"z\"}}\n"
            );
            fs::write(dir.join(format!("messages/{day}.jsonl")), line).unwrap();
        }
    }

    let v1 = run_node(
        a.path(),
        "compactOldFiles",
        json!({"opts": {"retentionDays": 14, "now": "2026-04-19T12:00:00.000Z"}}),
    );
    let c = FixedClock(FIXED_MS);
    let r2 = compact_old_files(
        b.path(),
        &CompactOpts {
            retention_days: Some(14.0),
            now: Some("2026-04-19T12:00:00.000Z"),
        },
        &c,
    );
    let v2 = json!({
        "deleted": r2.deleted,
        "kept": r2.kept,
        "retention_days": r2.retention_days,
        "cutoff": r2.cutoff,
    });
    assert_eq!(v1, v2);
    assert_eq!(read_messages(a.path()), read_messages(b.path()));
}

#[test]
fn compact_uses_config_default() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    for dir in [a.path(), b.path()] {
        fs::create_dir_all(dir.join("messages")).unwrap();
        for day in ["2026-03-15", "2026-04-19"] {
            let line = format!(
                "{{\"ts\":\"{day}T00:00:00.000Z\",\"type\":\"created\",\"trace_id\":\"z\"}}\n"
            );
            fs::write(dir.join(format!("messages/{day}.jsonl")), line).unwrap();
        }
    }

    let v1 = run_node(a.path(), "compactOldFiles", json!({"opts": {}}));
    let c = FixedClock(FIXED_MS);
    let r2 = compact_old_files(b.path(), &CompactOpts::default(), &c);
    let v2 = json!({
        "deleted": r2.deleted,
        "kept": r2.kept,
        "retention_days": r2.retention_days,
        "cutoff": r2.cutoff,
    });
    assert_eq!(v1, v2);
}

#[test]
fn get_config_defaults() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let v1 = run_node(a.path(), "getConfig", json!({}));
    let v2 = get_config(a.path());
    assert_eq!(v1, Value::Object(v2));
}

#[test]
fn set_config_persists() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let v1 = run_node(
        a.path(),
        "setConfig",
        json!({"patch": {"retention_days": 30}}),
    );
    let v2 = set_config(b.path(), &json!({"retention_days": 30})).unwrap();
    assert_eq!(v1, Value::Object(v2));

    // Arquivo no disco deve ser idêntico (JSON pretty sem \n final).
    let a_raw = fs::read_to_string(a.path().join("bus-config.json")).unwrap();
    let b_raw = fs::read_to_string(b.path().join("bus-config.json")).unwrap();
    assert_eq!(a_raw, b_raw);
}

#[test]
fn set_config_invalid_throws_same_message() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let v1 = run_node(
        a.path(),
        "setConfig",
        json!({"patch": {"retention_days": 400}}),
    );
    let err_v1 = v1["__error"].as_str().unwrap();
    let e = set_config(a.path(), &json!({"retention_days": 400})).unwrap_err();
    assert_eq!(err_v1, e.to_string());
}

#[test]
fn list_day_files_sorted() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    for dir in [a.path(), b.path()] {
        fs::create_dir_all(dir.join("messages")).unwrap();
        for day in ["2026-04-19", "2026-04-17", "2026-04-18"] {
            fs::write(dir.join(format!("messages/{day}.jsonl")), "").unwrap();
        }
        // Ruído ignorado.
        fs::write(dir.join("messages/README.md"), "").unwrap();
    }
    let v1 = run_node(a.path(), "listDayFiles", json!({}));
    let v2: Value = Value::Array(list_day_files(b.path()).into_iter().map(Value::String).collect());
    assert_eq!(v1, v2);
}
