//! Paridade v1 vs v2 para `saveUserPatterns` e `setCoreDisabled`:
//! verifica que o arquivo `destructive_patterns.json` gerado é idêntico
//! byte-a-byte nos dois lados. Para isolar não-determinismo, monkey-patch
//! de `Date` (created_at) e `crypto.randomBytes` (id) no Node.

use domain_destructive::{save_user_patterns, set_core_disabled, FixedIdGen, FixedIsoClock};
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::TempDir;

const FIXED_ISO: &str = "2026-04-19T12:00:00.000Z";
const FIXED_MS: i64 = 1_776_600_000_000;
const FIXED_HEX: &str = "deadbeefcafe"; // 6 bytes

/// Monkey-patch:
/// - `Date` (shim por new Date())
/// - `crypto.randomBytes(n)` → Buffer fixo de tamanho n.
const NODE_SCRIPT: &str = r#"
const cjsPath = process.argv[1];
const msFixed = Number(process.argv[3]);
const fixedHex = process.argv[4];
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

// Patch crypto.randomBytes antes do require.
const realCrypto = require('crypto');
const fixedBuf = Buffer.from(fixedHex, 'hex');
realCrypto.randomBytes = (n) => fixedBuf.subarray(0, n);

const lib = require(cjsPath);
let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', d => { input += d; });
process.stdin.on('end', () => {
  try {
    const { op, memDir, args } = JSON.parse(input);
    let out;
    if (op === 'saveUserPatterns') out = lib.saveUserPatterns(memDir, args.patterns);
    else if (op === 'setCoreDisabled') out = lib.setCoreDisabled(memDir, args.id, args.disabled);
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
        .arg("unused")
        .arg(FIXED_MS.to_string())
        .arg(FIXED_HEX)
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
        panic!(
            "node exited: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn save_single_pattern_byte_equal() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let patterns = json!([{
        "label": "Block prod wipe",
        "text": "wipe prod",
        "severity": "critical",
    }]);

    let v1 = run_node(a.path(), "saveUserPatterns", json!({"patterns": patterns}));
    let id_gen = FixedIdGen(format!("user.{FIXED_HEX}"));
    let clock = FixedIsoClock(FIXED_ISO.into());
    let v2 = save_user_patterns(
        b.path(),
        patterns.as_array().unwrap(),
        &id_gen,
        &clock,
    );
    assert_eq!(v1, Value::Array(v2));

    let a_raw = fs::read_to_string(a.path().join("destructive_patterns.json")).unwrap();
    let b_raw = fs::read_to_string(b.path().join("destructive_patterns.json")).unwrap();
    assert_eq!(a_raw, b_raw);
}

#[test]
fn save_with_fields_provided() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let patterns = json!([{
        "id": "user.custom",
        "label": "already set",
        "text": "already",
        "kind": "regex",
        "severity": "medium",
        "enabled": false,
        "created_at": "2026-01-01T00:00:00.000Z",
    }]);

    let v1 = run_node(a.path(), "saveUserPatterns", json!({"patterns": patterns}));
    let id_gen = FixedIdGen(format!("user.{FIXED_HEX}"));
    let clock = FixedIsoClock(FIXED_ISO.into());
    let v2 = save_user_patterns(
        b.path(),
        patterns.as_array().unwrap(),
        &id_gen,
        &clock,
    );
    assert_eq!(v1, Value::Array(v2));
    assert_eq!(
        fs::read_to_string(a.path().join("destructive_patterns.json")).unwrap(),
        fs::read_to_string(b.path().join("destructive_patterns.json")).unwrap()
    );
}

#[test]
fn save_drops_empty() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let patterns = json!([
        {"label":"","text":"foo"},
        {"label":"ok","text":""},
        {"label":"keep","text":"keep","severity":"high"},
    ]);
    let v1 = run_node(a.path(), "saveUserPatterns", json!({"patterns": patterns}));
    let id_gen = FixedIdGen(format!("user.{FIXED_HEX}"));
    let clock = FixedIsoClock(FIXED_ISO.into());
    let v2 = save_user_patterns(b.path(), patterns.as_array().unwrap(), &id_gen, &clock);
    assert_eq!(v1, Value::Array(v2));
    assert_eq!(
        fs::read_to_string(a.path().join("destructive_patterns.json")).unwrap(),
        fs::read_to_string(b.path().join("destructive_patterns.json")).unwrap()
    );
}

#[test]
fn save_truncates_long_strings() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let long_label = "x".repeat(250);
    let long_text = "y".repeat(500);
    let patterns = json!([{"label": long_label, "text": long_text}]);
    let v1 = run_node(a.path(), "saveUserPatterns", json!({"patterns": patterns}));
    let id_gen = FixedIdGen(format!("user.{FIXED_HEX}"));
    let clock = FixedIsoClock(FIXED_ISO.into());
    let v2 = save_user_patterns(b.path(), patterns.as_array().unwrap(), &id_gen, &clock);
    assert_eq!(v1, Value::Array(v2));
    assert_eq!(
        fs::read_to_string(a.path().join("destructive_patterns.json")).unwrap(),
        fs::read_to_string(b.path().join("destructive_patterns.json")).unwrap()
    );
}

#[test]
fn set_core_disabled_byte_equal() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();

    let v1 = run_node(
        a.path(),
        "setCoreDisabled",
        json!({"id": "core.rm_rf", "disabled": true}),
    );
    let v2 = set_core_disabled(b.path(), "core.rm_rf", true).unwrap();
    assert_eq!(v1, Value::Array(v2.iter().map(|s| Value::String(s.clone())).collect()));
    assert_eq!(
        fs::read_to_string(a.path().join("destructive_patterns.json")).unwrap(),
        fs::read_to_string(b.path().join("destructive_patterns.json")).unwrap()
    );
}

#[test]
fn set_core_disabled_idempotent() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    // Toggle on duas vezes — set dedupe.
    run_node(
        a.path(),
        "setCoreDisabled",
        json!({"id": "core.shutdown", "disabled": true}),
    );
    let v1 = run_node(
        a.path(),
        "setCoreDisabled",
        json!({"id": "core.shutdown", "disabled": true}),
    );
    set_core_disabled(b.path(), "core.shutdown", true).unwrap();
    let v2 = set_core_disabled(b.path(), "core.shutdown", true).unwrap();
    assert_eq!(v1, Value::Array(v2.iter().map(|s| Value::String(s.clone())).collect()));
    assert_eq!(
        fs::read_to_string(a.path().join("destructive_patterns.json")).unwrap(),
        fs::read_to_string(b.path().join("destructive_patterns.json")).unwrap()
    );
}

#[test]
fn set_core_disabled_preserves_user_patterns() {
    if !node_available() {
        eprintln!("skip: node missing");
        return;
    }
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    // Seed user patterns.
    let seed = json!({
        "patterns": [{
            "id": "user.existing",
            "label": "keep me",
            "text": "stay",
            "kind": "literal",
            "severity": "high",
            "enabled": true,
            "created_at": "2026-04-19T12:00:00.000Z",
        }],
        "disabled_core": [],
    });
    let content = serde_json::to_string_pretty(&seed).unwrap();
    fs::write(a.path().join("destructive_patterns.json"), &content).unwrap();
    fs::write(b.path().join("destructive_patterns.json"), &content).unwrap();

    run_node(
        a.path(),
        "setCoreDisabled",
        json!({"id": "core.drop_database", "disabled": true}),
    );
    set_core_disabled(b.path(), "core.drop_database", true).unwrap();
    assert_eq!(
        fs::read_to_string(a.path().join("destructive_patterns.json")).unwrap(),
        fs::read_to_string(b.path().join("destructive_patterns.json")).unwrap()
    );
}
