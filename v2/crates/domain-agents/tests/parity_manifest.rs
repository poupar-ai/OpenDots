//! Paridade v1 (`agents.cjs::loadManifest/saveManifest`) vs v2
//! (`domain_agents::manifest::{load_manifest, save_manifest}`).
//!
//! Dois dirs irmãos, operação em cada, comparação byte-a-byte do
//! `agents.json`.

use domain_agents::{load_manifest, manifest_path, save_manifest};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

const NODE_SCRIPT: &str = r#"
const lib = require(process.argv[1]);
const fn = process.argv[2];
const memDir = process.argv[3];
const arg = process.argv[4];
switch (fn) {
  case 'loadManifest': {
    const r = lib.loadManifest(memDir);
    process.stdout.write(r === null ? 'NULL' : JSON.stringify(r));
    break;
  }
  case 'saveManifest': {
    lib.saveManifest(memDir, JSON.parse(arg));
    process.stdout.write('');
    break;
  }
  default: throw new Error('unknown fn: ' + fn);
}
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

fn call_v1(fn_name: &str, mem_dir: &Path, arg: &str) -> String {
    let out = Command::new("node")
        .arg("-e")
        .arg(NODE_SCRIPT)
        .arg(lib_cjs_path())
        .arg(fn_name)
        .arg(mem_dir)
        .arg(arg)
        .output()
        .expect("spawn node");
    assert!(
        out.status.success(),
        "node falhou fn={fn_name}: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf8")
}

fn twin_dirs(tmp: &TempDir, initial: Option<&str>) -> (PathBuf, PathBuf) {
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();
    if let Some(content) = initial {
        fs::write(manifest_path(&a), content).unwrap();
        fs::write(manifest_path(&b), content).unwrap();
    }
    (a, b)
}

// ─── loadManifest ──────────────────────────────────────────────────────────

#[test]
fn parity_load_missing_returns_null() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, None);
    let v1 = call_v1("loadManifest", &a, "");
    let v2 = load_manifest(&b);
    assert_eq!(v1, "NULL");
    assert!(v2.is_none());
}

#[test]
fn parity_load_valid_manifest() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let initial = r#"{"version":1,"manager":"manager","agents":{"manager":{"type":"core"}}}"#;
    let (a, b) = twin_dirs(&tmp, Some(initial));
    let v1 = call_v1("loadManifest", &a, "");
    let v2 = load_manifest(&b).unwrap();
    assert_eq!(v1, serde_json::to_string(&v2).unwrap());
}

#[test]
fn parity_load_corrupt_returns_null() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, Some("{not json"));
    let v1 = call_v1("loadManifest", &a, "");
    let v2 = load_manifest(&b);
    assert_eq!(v1, "NULL");
    assert!(v2.is_none());
}

#[test]
fn parity_load_empty_file_returns_null() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, Some(""));
    let v1 = call_v1("loadManifest", &a, "");
    let v2 = load_manifest(&b);
    assert_eq!(v1, "NULL");
    assert!(v2.is_none());
}

// ─── saveManifest ──────────────────────────────────────────────────────────

#[test]
fn parity_save_bytes_match() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, None);
    let cfg = json!({"version": 1, "manager": "manager", "agents": {}});
    let arg = serde_json::to_string(&cfg).unwrap();
    call_v1("saveManifest", &a, &arg);
    save_manifest(&b, &cfg).unwrap();
    assert_eq!(
        fs::read(manifest_path(&a)).unwrap(),
        fs::read(manifest_path(&b)).unwrap()
    );
}

#[test]
fn parity_save_trailing_newline() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, None);
    let cfg = json!({"x": 1});
    let arg = serde_json::to_string(&cfg).unwrap();
    call_v1("saveManifest", &a, &arg);
    save_manifest(&b, &cfg).unwrap();
    let a_bytes = fs::read(manifest_path(&a)).unwrap();
    let b_bytes = fs::read(manifest_path(&b)).unwrap();
    assert_eq!(a_bytes.last(), Some(&b'\n'));
    assert_eq!(a_bytes, b_bytes);
}

#[test]
fn parity_save_preserves_key_order() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, None);
    // Order z, a, m — JS preserva; serde preserve_order também.
    let raw = r#"{"z":1,"a":2,"m":{"inner_z":true,"inner_a":false}}"#;
    call_v1("saveManifest", &a, raw);
    let cfg: Value = serde_json::from_str(raw).unwrap();
    save_manifest(&b, &cfg).unwrap();
    assert_eq!(
        fs::read(manifest_path(&a)).unwrap(),
        fs::read(manifest_path(&b)).unwrap()
    );
}

#[test]
fn parity_save_with_nested_arrays() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, None);
    let cfg = json!({
        "agents": {
            "manager": {
                "tools": ["Task", "Read", "Write"],
                "can_talk_to": ["oracle", "architect"]
            }
        }
    });
    let arg = serde_json::to_string(&cfg).unwrap();
    call_v1("saveManifest", &a, &arg);
    save_manifest(&b, &cfg).unwrap();
    assert_eq!(
        fs::read(manifest_path(&a)).unwrap(),
        fs::read(manifest_path(&b)).unwrap()
    );
}

#[test]
fn parity_save_unicode() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, None);
    let cfg = json!({
        "rule": "Toda decisão relevante deve ser comunicada ao Manager — sem exceções.",
        "emoji": "🎯"
    });
    let arg = serde_json::to_string(&cfg).unwrap();
    call_v1("saveManifest", &a, &arg);
    save_manifest(&b, &cfg).unwrap();
    assert_eq!(
        fs::read(manifest_path(&a)).unwrap(),
        fs::read(manifest_path(&b)).unwrap()
    );
}

#[test]
fn parity_save_overwrites_previous() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, Some(r#"{"old":true}"#));
    let cfg = json!({"new": true});
    let arg = serde_json::to_string(&cfg).unwrap();
    call_v1("saveManifest", &a, &arg);
    save_manifest(&b, &cfg).unwrap();
    assert_eq!(
        fs::read(manifest_path(&a)).unwrap(),
        fs::read(manifest_path(&b)).unwrap()
    );
}

// ─── roundtrip ────────────────────────────────────────────────────────────

#[test]
fn parity_roundtrip_save_then_load() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, None);
    let cfg = json!({
        "version": 1,
        "manager": "manager",
        "agents": {
            "manager": {"type": "core", "autonomy": {"level": "high"}}
        }
    });
    let arg = serde_json::to_string(&cfg).unwrap();
    call_v1("saveManifest", &a, &arg);
    save_manifest(&b, &cfg).unwrap();
    let a_load = call_v1("loadManifest", &a, "");
    let b_load = load_manifest(&b).unwrap();
    assert_eq!(a_load, serde_json::to_string(&b_load).unwrap());
}
