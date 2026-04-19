//! Paridade v1 (`plugin/scripts/lib.cjs :: loadStats/saveStats/incrementStats`)
//! vs v2 (`core_paths::stats`).
//!
//! Estratégia: dois diretórios irmãos (`a/` p/ v1, `b/` p/ v2). Operamos em
//! paralelo e comparamos:
//!
//! 1. O **conteúdo byte-a-byte** de `stats.json` gerado.
//! 2. O **retorno JSON** de `loadStats`/`incrementStats`.
//!
//! Cada caso monta um estado inicial idêntico nos dois dirs, aplica a
//! operação, e faz `assert_eq` no hex dump do arquivo.

use core_paths::{increment_stats, load_stats, save_stats, STATS_FILE};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

const NODE_SCRIPT: &str = r#"
const lib = require(process.argv[1]);
const fn = process.argv[2];
const memDir = process.argv[3];
const arg = process.argv[4];
let out;
switch (fn) {
  case 'loadStats': {
    const r = lib.loadStats(memDir);
    out = r === null ? 'NULL' : JSON.stringify(r);
    break;
  }
  case 'saveStats': {
    const stats = JSON.parse(arg);
    lib.saveStats(memDir, stats);
    out = '';
    break;
  }
  case 'incrementStats': {
    const r = lib.incrementStats(memDir, arg);
    out = JSON.stringify(r);
    break;
  }
  default: throw new Error('unknown fn: ' + fn);
}
process.stdout.write(out);
"#;

fn lib_cjs_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../plugin/scripts/lib.cjs")
        .canonicalize()
        .expect("lib.cjs missing")
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

/// Monta dois dirs irmãos com o mesmo estado inicial e retorna `(v1_dir, v2_dir)`.
fn twin_dirs(tmp: &TempDir, initial_file: Option<&str>) -> (PathBuf, PathBuf) {
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();
    if let Some(content) = initial_file {
        fs::write(a.join(STATS_FILE), content).unwrap();
        fs::write(b.join(STATS_FILE), content).unwrap();
    }
    (a, b)
}

fn read_bytes(p: &Path) -> Vec<u8> {
    fs::read(p).expect("read stats.json")
}

// ─── load_stats ────────────────────────────────────────────────────────────

#[test]
fn parity_load_missing_returns_null() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, None);
    let v1 = call_v1("loadStats", &a, "");
    let v2 = load_stats(&b);
    assert_eq!(v1, "NULL");
    assert!(v2.is_none());
}

#[test]
fn parity_load_valid_object() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let initial = r#"{"sessions": 7, "observations": 42}"#;
    let (a, b) = twin_dirs(&tmp, Some(initial));
    let v1 = call_v1("loadStats", &a, "");
    let v2 = load_stats(&b).unwrap();
    // Comparamos via serialização canônica (to_string sem pretty — ordem preservada).
    assert_eq!(v1, serde_json::to_string(&v2).unwrap());
}

#[test]
fn parity_load_corrupt_returns_null() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, Some("{not json"));
    let v1 = call_v1("loadStats", &a, "");
    let v2 = load_stats(&b);
    assert_eq!(v1, "NULL");
    assert!(v2.is_none());
}

// ─── save_stats ────────────────────────────────────────────────────────────

#[test]
fn parity_save_simple_object_bytes_match() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, None);
    let payload = r#"{"sessions":3,"observations":12}"#;
    call_v1("saveStats", &a, payload);
    save_stats(&b, &serde_json::from_str(payload).unwrap());
    assert_eq!(read_bytes(&a.join(STATS_FILE)), read_bytes(&b.join(STATS_FILE)));
}

#[test]
fn parity_save_with_heterogeneous_fields() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, None);
    let payload = r#"{
      "sessions":5,
      "observations":10,
      "last_injection_tokens":3200,
      "last_injection_budget":8000,
      "last_injection_at":"2026-04-19T03:00:00.000Z",
      "last_injection_blocks":["recap:120","rules:80"],
      "last_injection_skipped":[]
    }"#;
    call_v1("saveStats", &a, payload);
    save_stats(&b, &serde_json::from_str(payload).unwrap());
    assert_eq!(read_bytes(&a.join(STATS_FILE)), read_bytes(&b.join(STATS_FILE)));
}

#[test]
fn parity_save_empty_object() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, None);
    call_v1("saveStats", &a, "{}");
    save_stats(&b, &serde_json::json!({}));
    assert_eq!(read_bytes(&a.join(STATS_FILE)), read_bytes(&b.join(STATS_FILE)));
}

#[test]
fn parity_save_preserves_insertion_order() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, None);
    let payload = r#"{"zeta":1,"alpha":2,"beta":3}"#;
    call_v1("saveStats", &a, payload);
    save_stats(&b, &serde_json::from_str(payload).unwrap());
    assert_eq!(read_bytes(&a.join(STATS_FILE)), read_bytes(&b.join(STATS_FILE)));
}

// ─── increment_stats ────────────────────────────────────────────────────────

#[test]
fn parity_increment_from_scratch() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, None);
    let v1 = call_v1("incrementStats", &a, "sessions");
    let v2 = increment_stats(&b, "sessions");
    assert_eq!(v1, serde_json::to_string(&v2).unwrap());
    assert_eq!(read_bytes(&a.join(STATS_FILE)), read_bytes(&b.join(STATS_FILE)));
}

#[test]
fn parity_increment_observations_from_scratch() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, None);
    call_v1("incrementStats", &a, "observations");
    increment_stats(&b, "observations");
    assert_eq!(read_bytes(&a.join(STATS_FILE)), read_bytes(&b.join(STATS_FILE)));
}

#[test]
fn parity_increment_over_existing_stats() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let initial = "{\n  \"sessions\": 5,\n  \"observations\": 10\n}";
    let (a, b) = twin_dirs(&tmp, Some(initial));
    call_v1("incrementStats", &a, "sessions");
    increment_stats(&b, "sessions");
    assert_eq!(read_bytes(&a.join(STATS_FILE)), read_bytes(&b.join(STATS_FILE)));
}

#[test]
fn parity_increment_new_field_appends() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let initial = "{\n  \"sessions\": 0,\n  \"observations\": 0\n}";
    let (a, b) = twin_dirs(&tmp, Some(initial));
    call_v1("incrementStats", &a, "custom_counter");
    increment_stats(&b, "custom_counter");
    assert_eq!(read_bytes(&a.join(STATS_FILE)), read_bytes(&b.join(STATS_FILE)));
}

#[test]
fn parity_increment_over_corrupt_file() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, Some("not json at all"));
    call_v1("incrementStats", &a, "observations");
    increment_stats(&b, "observations");
    assert_eq!(read_bytes(&a.join(STATS_FILE)), read_bytes(&b.join(STATS_FILE)));
}

#[test]
fn parity_increment_twice_accumulates() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, None);
    for _ in 0..5 {
        call_v1("incrementStats", &a, "sessions");
        increment_stats(&b, "sessions");
    }
    assert_eq!(read_bytes(&a.join(STATS_FILE)), read_bytes(&b.join(STATS_FILE)));
}

#[test]
fn parity_increment_with_null_field_value() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let initial = "{\n  \"sessions\": null,\n  \"observations\": 0\n}";
    let (a, b) = twin_dirs(&tmp, Some(initial));
    call_v1("incrementStats", &a, "sessions");
    increment_stats(&b, "sessions");
    assert_eq!(read_bytes(&a.join(STATS_FILE)), read_bytes(&b.join(STATS_FILE)));
}

#[test]
fn parity_increment_preserves_other_fields_intact() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let initial = r#"{
  "sessions": 3,
  "observations": 8,
  "last_injection_at": "2026-04-19T03:00:00.000Z",
  "last_injection_blocks": ["recap:10","rules:5"]
}"#;
    let (a, b) = twin_dirs(&tmp, Some(initial));
    call_v1("incrementStats", &a, "sessions");
    increment_stats(&b, "sessions");
    assert_eq!(read_bytes(&a.join(STATS_FILE)), read_bytes(&b.join(STATS_FILE)));
}
