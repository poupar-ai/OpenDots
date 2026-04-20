//! Paridade v1 (`plugin/scripts/lib.cjs :: loadProjectConfig / saveProjectConfig
//! / initProjectConfig`) vs v2 (`core_paths::config`).
//!
//! Dois dirs irmãos (`a/` = v1, `b/` = v2), operações em paralelo,
//! comparação byte-a-byte do `config.json` gerado.
//!
//! Para `initProjectConfig` o `.cjs` usa `new Date().toISOString()`
//! internamente — stubamos `Date.prototype.toISOString` no script node
//! para fixar o timestamp e comparar com o Rust que recebe o mesmo ISO
//! como parâmetro.

use core_paths::{init_project_config, load_project_config, save_project_config, CONFIG_FILE};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

/// Script genérico que expõe as 3 funções do `.cjs`.
///
/// O stub de `Date.prototype.toISOString` é aplicado logo no topo — o
/// `.cjs` ainda faz `new Date().toISOString()` por dentro, mas o stub
/// captura a chamada e devolve `fixedISO`.
const NODE_SCRIPT: &str = r#"
const FIXED_ISO = process.argv[5] || '2026-04-19T03:00:00.000Z';
const _origISO = Date.prototype.toISOString;
Date.prototype.toISOString = function() { return FIXED_ISO; };

const lib = require(process.argv[1]);
const fn = process.argv[2];
const memDir = process.argv[3];
const arg = process.argv[4];
let out;
switch (fn) {
  case 'loadProjectConfig': {
    const r = lib.loadProjectConfig(memDir);
    out = r === null ? 'NULL' : JSON.stringify(r);
    break;
  }
  case 'saveProjectConfig': {
    lib.saveProjectConfig(memDir, JSON.parse(arg));
    out = '';
    break;
  }
  case 'initProjectConfig': {
    const r = lib.initProjectConfig(memDir, arg);
    out = JSON.stringify(r);
    break;
  }
  default: throw new Error('unknown fn: ' + fn);
}
process.stdout.write(out);
"#;

const FIXED_ISO: &str = "2026-04-19T03:00:00.000Z";

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
        .arg(FIXED_ISO)
        .output()
        .expect("spawn node");
    assert!(
        out.status.success(),
        "node falhou fn={fn_name}: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf8")
}

fn twin_dirs(tmp: &TempDir, initial_file: Option<&str>) -> (PathBuf, PathBuf) {
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();
    if let Some(content) = initial_file {
        fs::write(a.join(CONFIG_FILE), content).unwrap();
        fs::write(b.join(CONFIG_FILE), content).unwrap();
    }
    (a, b)
}

fn read_bytes(p: &Path) -> Vec<u8> {
    fs::read(p).expect("read config.json")
}

// ─── loadProjectConfig ─────────────────────────────────────────────────────

#[test]
fn parity_load_missing_returns_null() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, None);
    let v1 = call_v1("loadProjectConfig", &a, "");
    let v2 = load_project_config(&b);
    assert_eq!(v1, "NULL");
    assert!(v2.is_none());
}

#[test]
fn parity_load_valid_config() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let initial = r#"{"version":"1","project":"foo","created_at":"2026-01-01T00:00:00.000Z"}"#;
    let (a, b) = twin_dirs(&tmp, Some(initial));
    let v1 = call_v1("loadProjectConfig", &a, "");
    let v2 = load_project_config(&b).unwrap();
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
    let v1 = call_v1("loadProjectConfig", &a, "");
    let v2 = load_project_config(&b);
    assert_eq!(v1, "NULL");
    assert!(v2.is_none());
}

// ─── saveProjectConfig ─────────────────────────────────────────────────────

#[test]
fn parity_save_bytes_match_with_trailing_newline() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, None);
    let payload = r#"{"version":"1","project":"bankin","created_at":"2026-04-19T03:00:00.000Z"}"#;
    call_v1("saveProjectConfig", &a, payload);
    save_project_config(&b, &serde_json::from_str(payload).unwrap()).unwrap();
    let a_bytes = read_bytes(&a.join(CONFIG_FILE));
    let b_bytes = read_bytes(&b.join(CONFIG_FILE));
    assert_eq!(a_bytes, b_bytes);
    // Ambos terminam em '\n'.
    assert_eq!(a_bytes.last().copied(), Some(b'\n'));
}

#[test]
fn parity_save_heterogeneous_cfg() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, None);
    let payload = r#"{
      "version": "1",
      "project": "foo",
      "tags": ["a","b","c"],
      "meta": {"deep": {"ok": true}}
    }"#;
    call_v1("saveProjectConfig", &a, payload);
    save_project_config(&b, &serde_json::from_str(payload).unwrap()).unwrap();
    assert_eq!(read_bytes(&a.join(CONFIG_FILE)), read_bytes(&b.join(CONFIG_FILE)));
}

#[test]
fn parity_save_empty_cfg() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, None);
    call_v1("saveProjectConfig", &a, "{}");
    save_project_config(&b, &serde_json::json!({})).unwrap();
    assert_eq!(read_bytes(&a.join(CONFIG_FILE)), read_bytes(&b.join(CONFIG_FILE)));
}

// ─── initProjectConfig ─────────────────────────────────────────────────────

#[test]
fn parity_init_creates_when_missing() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, None);
    let v1 = call_v1("initProjectConfig", &a, "bankin");
    let v2 = init_project_config(&b, "bankin", FIXED_ISO).unwrap();
    assert_eq!(v1, serde_json::to_string(&v2).unwrap());
    assert_eq!(read_bytes(&a.join(CONFIG_FILE)), read_bytes(&b.join(CONFIG_FILE)));
}

#[test]
fn parity_init_is_idempotent_returns_existing() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let initial = r#"{
  "version": "1",
  "project": "previous",
  "created_at": "2020-01-01T00:00:00.000Z"
}
"#;
    let (a, b) = twin_dirs(&tmp, Some(initial));
    let v1 = call_v1("initProjectConfig", &a, "nome-novo");
    let v2 = init_project_config(&b, "nome-novo", FIXED_ISO).unwrap();
    // Idempotente: ambos devolvem o objeto existente (com `previous`/2020).
    assert_eq!(v1, serde_json::to_string(&v2).unwrap());
    assert!(v1.contains("previous"));
    // Arquivo preservado sem alterações nos dois lados.
    assert_eq!(read_bytes(&a.join(CONFIG_FILE)), read_bytes(&b.join(CONFIG_FILE)));
    assert_eq!(read_bytes(&a.join(CONFIG_FILE)), initial.as_bytes());
}

#[test]
fn parity_init_project_name_with_special_chars() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, None);
    // Nome com acentos, hífen, números.
    let name = "bankin-café-123";
    call_v1("initProjectConfig", &a, name);
    init_project_config(&b, name, FIXED_ISO).unwrap();
    assert_eq!(read_bytes(&a.join(CONFIG_FILE)), read_bytes(&b.join(CONFIG_FILE)));
}

#[test]
fn parity_init_overwrites_when_existing_is_json_null() {
    // config.json = "null" literal. JS: `null` falsy → overwrite.
    // Se Rust só checasse `Option::Some`, divergiria aqui (preserva o null).
    // Corrigido no Ponto 6 adicionando `is_js_truthy`.
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, Some("null"));
    call_v1("initProjectConfig", &a, "novo");
    init_project_config(&b, "novo", FIXED_ISO).unwrap();
    assert_eq!(read_bytes(&a.join(CONFIG_FILE)), read_bytes(&b.join(CONFIG_FILE)));
}

#[test]
fn parity_init_overwrites_when_existing_is_zero() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, Some("0"));
    call_v1("initProjectConfig", &a, "novo");
    init_project_config(&b, "novo", FIXED_ISO).unwrap();
    assert_eq!(read_bytes(&a.join(CONFIG_FILE)), read_bytes(&b.join(CONFIG_FILE)));
}

#[test]
fn parity_init_overwrites_when_existing_is_false() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, Some("false"));
    call_v1("initProjectConfig", &a, "novo");
    init_project_config(&b, "novo", FIXED_ISO).unwrap();
    assert_eq!(read_bytes(&a.join(CONFIG_FILE)), read_bytes(&b.join(CONFIG_FILE)));
}

#[test]
fn parity_init_overwrites_when_existing_is_empty_string() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, Some("\"\"")).clone();
    call_v1("initProjectConfig", &a, "novo");
    init_project_config(&b, "novo", FIXED_ISO).unwrap();
    assert_eq!(read_bytes(&a.join(CONFIG_FILE)), read_bytes(&b.join(CONFIG_FILE)));
}

#[test]
fn parity_init_preserves_when_existing_is_truthy_string() {
    // String não-vazia é truthy — .cjs retorna a string como-está.
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, Some("\"hello\""));
    let v1 = call_v1("initProjectConfig", &a, "novo");
    let v2 = init_project_config(&b, "novo", FIXED_ISO).unwrap();
    assert_eq!(v1, serde_json::to_string(&v2).unwrap());
    assert_eq!(read_bytes(&a.join(CONFIG_FILE)), read_bytes(&b.join(CONFIG_FILE)));
}

#[test]
fn parity_init_preserves_when_existing_is_empty_object() {
    // {} é truthy em JS (armadilha clássica — diferente de Python).
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, Some("{}"));
    let v1 = call_v1("initProjectConfig", &a, "novo");
    let v2 = init_project_config(&b, "novo", FIXED_ISO).unwrap();
    assert_eq!(v1, serde_json::to_string(&v2).unwrap());
    assert_eq!(read_bytes(&a.join(CONFIG_FILE)), read_bytes(&b.join(CONFIG_FILE)));
}

#[test]
fn parity_init_over_corrupt_overwrites() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp, Some("not json"));
    call_v1("initProjectConfig", &a, "novo");
    init_project_config(&b, "novo", FIXED_ISO).unwrap();
    // Ambos sobrescreveram o arquivo corrompido com a config nova (paridade
    // da "perda silenciosa" — bug latente a ser registrado em
    // migration_findings.md como F2-13).
    assert_eq!(read_bytes(&a.join(CONFIG_FILE)), read_bytes(&b.join(CONFIG_FILE)));
}
