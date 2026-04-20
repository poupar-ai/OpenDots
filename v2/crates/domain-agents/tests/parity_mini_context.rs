//! Paridade v1 (`agents.cjs::loadMiniContext/saveMiniContext`) vs v2
//! (`domain_agents::mini_context::*`).

use domain_agents::{load_mini_context, save_mini_context, MiniContextError};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::TempDir;

/// Script que lê argv[2]=fn, argv[3]=memDir, argv[4]=arg1. Se
/// argv[5] === '@stdin', arg2 = stdin bruto (pra contornar limite de
/// argv em OS — evita "Argument list too long" em content > ~120KB).
const NODE_SCRIPT: &str = r#"
const fs = require('fs');
const lib = require(process.argv[1]);
const fn = process.argv[2];
const memDir = process.argv[3];
const arg1 = process.argv[4];
const arg2Marker = process.argv[5];
const arg2 = arg2Marker === '@stdin' ? fs.readFileSync(0, 'utf8') : arg2Marker;
let out;
try {
  switch (fn) {
    case 'loadMiniContext': {
      const r = lib.loadMiniContext(memDir, arg1);
      out = r === null ? 'NULL' : 'OK:' + r;
      break;
    }
    case 'saveMiniContext': {
      const r = lib.saveMiniContext(memDir, arg1, arg2);
      out = 'OK:' + r;
      break;
    }
    default: throw new Error('unknown fn: ' + fn);
  }
} catch (e) {
  out = 'ERR:' + e.message;
}
process.stdout.write(out);
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

fn call_v1(fn_name: &str, mem_dir: &Path, arg1: &str, arg2: &str) -> String {
    let out = Command::new("node")
        .arg("-e")
        .arg(NODE_SCRIPT)
        .arg(lib_cjs_path())
        .arg(fn_name)
        .arg(mem_dir)
        .arg(arg1)
        .arg(arg2)
        .output()
        .expect("spawn node");
    assert!(
        out.status.success(),
        "node falhou fn={fn_name}: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf8")
}

/// Variante que passa `arg2` via stdin (evita limite do argv em
/// payloads grandes, > ~120KB no Linux).
fn call_v1_stdin(fn_name: &str, mem_dir: &Path, arg1: &str, stdin_arg: &str) -> String {
    let mut child = Command::new("node")
        .arg("-e")
        .arg(NODE_SCRIPT)
        .arg(lib_cjs_path())
        .arg(fn_name)
        .arg(mem_dir)
        .arg(arg1)
        .arg("@stdin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn node");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin_arg.as_bytes())
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    assert!(
        out.status.success(),
        "node falhou fn={fn_name}: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf8")
}

fn twin_dirs(tmp: &TempDir) -> (PathBuf, PathBuf) {
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();
    (a, b)
}

// ─── loadMiniContext ──────────────────────────────────────────────────────

#[test]
fn parity_load_missing() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let v1 = call_v1("loadMiniContext", &a, "x.md", "");
    let v2 = load_mini_context(&b, "x.md");
    assert_eq!(v1, "NULL");
    assert!(v2.is_none());
}

#[test]
fn parity_load_plain_text() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    fs::write(a.join("x.md"), "hello world").unwrap();
    fs::write(b.join("x.md"), "hello world").unwrap();
    let v1 = call_v1("loadMiniContext", &a, "x.md", "");
    let v2 = load_mini_context(&b, "x.md").unwrap();
    assert_eq!(v1, format!("OK:{v2}"));
}

#[test]
fn parity_load_trims_whitespace() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let raw = "\n\n  hello  \n\n";
    fs::write(a.join("x.md"), raw).unwrap();
    fs::write(b.join("x.md"), raw).unwrap();
    let v1 = call_v1("loadMiniContext", &a, "x.md", "");
    let v2 = load_mini_context(&b, "x.md").unwrap();
    assert_eq!(v1, format!("OK:{v2}"));
    assert_eq!(v2, "hello");
}

#[test]
fn parity_load_trims_bom() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let raw = "\u{FEFF}hello\n";
    fs::write(a.join("x.md"), raw).unwrap();
    fs::write(b.join("x.md"), raw).unwrap();
    let v1 = call_v1("loadMiniContext", &a, "x.md", "");
    let v2 = load_mini_context(&b, "x.md").unwrap();
    assert_eq!(v1, format!("OK:{v2}"));
    assert_eq!(v2, "hello");
}

#[test]
fn parity_load_nested_path() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    fs::create_dir_all(a.join("agents")).unwrap();
    fs::create_dir_all(b.join("agents")).unwrap();
    fs::write(a.join("agents/manager.md"), "# Manager\n\nrole text").unwrap();
    fs::write(b.join("agents/manager.md"), "# Manager\n\nrole text").unwrap();
    let v1 = call_v1("loadMiniContext", &a, "agents/manager.md", "");
    let v2 = load_mini_context(&b, "agents/manager.md").unwrap();
    assert_eq!(v1, format!("OK:{v2}"));
}

#[test]
fn parity_load_empty_file() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    fs::write(a.join("x.md"), "").unwrap();
    fs::write(b.join("x.md"), "").unwrap();
    let v1 = call_v1("loadMiniContext", &a, "x.md", "");
    let v2 = load_mini_context(&b, "x.md").unwrap();
    assert_eq!(v1, "OK:");
    assert_eq!(v2, "");
}

// ─── saveMiniContext — validação ───────────────────────────────────────────

#[test]
fn parity_save_rejects_empty() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let v1 = call_v1("saveMiniContext", &a, "", "body");
    let v2 = save_mini_context(&b, "", "body").unwrap_err();
    assert_eq!(v1, "ERR:context_file inválido");
    assert!(matches!(v2, MiniContextError::InvalidContextFile));
}

#[test]
fn parity_save_rejects_absolute() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let v1 = call_v1("saveMiniContext", &a, "/etc/passwd", "body");
    let v2 = save_mini_context(&b, "/etc/passwd", "body").unwrap_err();
    assert_eq!(v1, "ERR:context_file inválido");
    assert!(matches!(v2, MiniContextError::InvalidContextFile));
}

#[test]
fn parity_save_rejects_dotdot() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    for rel in ["../x.md", "safe/../escape.md", "my..file.md", "a/../b.md"] {
        let v1 = call_v1("saveMiniContext", &a, rel, "body");
        let v2 = save_mini_context(&b, rel, "body").unwrap_err();
        assert_eq!(v1, "ERR:context_file inválido", "rel={rel}");
        assert!(matches!(v2, MiniContextError::InvalidContextFile));
    }
}

#[test]
fn parity_save_rejects_content_too_long() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let big = "a".repeat(200_001);
    // Passa via stdin — 200KB excede limite de argv em Linux.
    let v1 = call_v1_stdin("saveMiniContext", &a, "x.md", &big);
    let v2 = save_mini_context(&b, "x.md", &big).unwrap_err();
    assert_eq!(v1, "ERR:mini-contexto muito longo (máx 200KB)");
    assert!(matches!(v2, MiniContextError::TooLong));
}

#[test]
fn parity_save_accepts_exactly_200k() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let content = "a".repeat(200_000);
    let v1 = call_v1_stdin("saveMiniContext", &a, "x.md", &content);
    save_mini_context(&b, "x.md", &content).unwrap();
    assert!(v1.starts_with("OK:"));
    assert_eq!(
        fs::read(a.join("x.md")).unwrap(),
        fs::read(b.join("x.md")).unwrap()
    );
}

// ─── saveMiniContext — bytes na saída ─────────────────────────────────────

#[test]
fn parity_save_plain_text_bytes_match() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    call_v1("saveMiniContext", &a, "agents/x.md", "body");
    save_mini_context(&b, "agents/x.md", "body").unwrap();
    assert_eq!(
        fs::read(a.join("agents/x.md")).unwrap(),
        fs::read(b.join("agents/x.md")).unwrap()
    );
    // Trailing \n garantido em ambos.
    let bytes = fs::read(a.join("agents/x.md")).unwrap();
    assert_eq!(bytes.last(), Some(&b'\n'));
}

#[test]
fn parity_save_does_not_double_newline() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    call_v1("saveMiniContext", &a, "x.md", "body\n");
    save_mini_context(&b, "x.md", "body\n").unwrap();
    assert_eq!(
        fs::read(a.join("x.md")).unwrap(),
        fs::read(b.join("x.md")).unwrap()
    );
    let bytes = fs::read(a.join("x.md")).unwrap();
    assert_eq!(bytes, b"body\n");
}

#[test]
fn parity_save_creates_nested_dir() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    call_v1("saveMiniContext", &a, "deep/nested/x.md", "body");
    save_mini_context(&b, "deep/nested/x.md", "body").unwrap();
    assert!(a.join("deep/nested").is_dir());
    assert!(b.join("deep/nested").is_dir());
    assert_eq!(
        fs::read(a.join("deep/nested/x.md")).unwrap(),
        fs::read(b.join("deep/nested/x.md")).unwrap()
    );
}

#[test]
fn parity_save_unicode_content() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    let content = "# Manager\n\nRegra de ouro: Nunca mexa em produção sem aprovação — jamais. 🎯";
    call_v1("saveMiniContext", &a, "agents/manager.md", content);
    save_mini_context(&b, "agents/manager.md", content).unwrap();
    assert_eq!(
        fs::read(a.join("agents/manager.md")).unwrap(),
        fs::read(b.join("agents/manager.md")).unwrap()
    );
}

// ─── roundtrip ────────────────────────────────────────────────────────────

#[test]
fn parity_roundtrip_save_then_load() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (a, b) = twin_dirs(&tmp);
    call_v1("saveMiniContext", &a, "agents/x.md", "my content");
    save_mini_context(&b, "agents/x.md", "my content").unwrap();
    let v1 = call_v1("loadMiniContext", &a, "agents/x.md", "");
    let v2 = load_mini_context(&b, "agents/x.md").unwrap();
    assert_eq!(v1, format!("OK:{v2}"));
}
