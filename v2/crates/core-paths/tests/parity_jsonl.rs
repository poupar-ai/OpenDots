//! Paridade v1 (`plugin/scripts/lib.cjs`) vs v2 (core-paths::jsonl):
//! - `readJsonl(file)`
//! - `appendJsonl(file, obj)`
//! - `readLastJsonlLine(file)`
//!
//! Cada cenário é montado em dois diretórios irmãos (`a/`, `b/`); o
//! v1 opera no primeiro (via `node -e`), v2 no segundo. Comparamos o
//! conteúdo do arquivo gerado e o JSON retornado byte-a-byte.

use core_paths::{append_jsonl, read_jsonl, read_last_jsonl_line};
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

const NODE_SCRIPT: &str = r#"
const fs = require('fs');
const lib = require(process.argv[1]);
const fn = process.argv[2];
const arg = process.argv[3];
const payload = process.argv[4];
let out;
switch (fn) {
  case 'readJsonl':
    out = JSON.stringify(lib.readJsonl(arg));
    break;
  case 'appendJsonl': {
    const obj = JSON.parse(payload);
    lib.appendJsonl(arg, obj);
    out = '';
    break;
  }
  case 'readLastJsonlLine': {
    const r = lib.readLastJsonlLine(arg);
    out = r === null ? 'NULL' : JSON.stringify(r);
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

fn call_v1(fn_name: &str, arg: &Path, payload: &str) -> String {
    let out = Command::new("node")
        .arg("-e")
        .arg(NODE_SCRIPT)
        .arg(lib_cjs_path())
        .arg(fn_name)
        .arg(arg)
        .arg(payload)
        .output()
        .expect("failed to spawn node");
    assert!(
        out.status.success(),
        "node exited {:?}, stderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

// ─── readJsonl ─────────────────────────────────────────────────────────────

#[test]
fn read_jsonl_missing_file_matches_v1() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let p = tmp.path().join("none.jsonl");
    let v1 = call_v1("readJsonl", &p, "");
    let v2 = serde_json::to_string(&read_jsonl(&p).unwrap()).unwrap();
    assert_eq!(v1, v2);
}

#[test]
fn read_jsonl_mixed_content_matches_v1() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let p = tmp.path().join("mix.jsonl");
    // blanks, linhas inválidas, valores falsy — stress do filter(Boolean)
    std::fs::write(
        &p,
        "{\"a\":1}\n\n  \nnope\n{\"b\":2}\nnull\nfalse\n0\n\"\"\n{\"c\":3}\n",
    )
    .unwrap();
    let v1 = call_v1("readJsonl", &p, "");
    let v2 = serde_json::to_string(&read_jsonl(&p).unwrap()).unwrap();
    assert_eq!(v1, v2);
}

#[test]
fn read_jsonl_no_trailing_newline_matches_v1() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let p = tmp.path().join("no_nl.jsonl");
    std::fs::write(&p, "{\"a\":1}\n{\"b\":2}").unwrap(); // sem \n final
    let v1 = call_v1("readJsonl", &p, "");
    let v2 = serde_json::to_string(&read_jsonl(&p).unwrap()).unwrap();
    assert_eq!(v1, v2);
}

#[test]
fn read_jsonl_crlf_line_endings_matches_v1() {
    // Arquivo gerado em Windows: \r\n. .cjs faz split('\n') → \r fica
    // na cauda. trim() remove \r no filter, mas JSON.parse é chamado
    // na linha SEM trim — precisa funcionar com trailing \r.
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let p = tmp.path().join("crlf.jsonl");
    std::fs::write(&p, "{\"a\":1}\r\n{\"b\":2}\r\n").unwrap();
    let v1 = call_v1("readJsonl", &p, "");
    let v2 = serde_json::to_string(&read_jsonl(&p).unwrap()).unwrap();
    assert_eq!(v1, v2);
}

// ─── appendJsonl ───────────────────────────────────────────────────────────

#[test]
fn append_jsonl_produces_same_bytes_as_v1() {
    if !node_available() {
        return;
    }
    let a_dir = TempDir::new().unwrap();
    let b_dir = TempDir::new().unwrap();
    let a = a_dir.path().join("f.jsonl");
    let b = b_dir.path().join("f.jsonl");
    let payload = r#"{"id":42,"msg":"hi","tags":["x","y"]}"#;

    call_v1("appendJsonl", &a, payload);
    append_jsonl(&b, &serde_json::from_str(payload).unwrap()).unwrap();

    let a_bytes = std::fs::read(&a).unwrap();
    let b_bytes = std::fs::read(&b).unwrap();
    assert_eq!(
        a_bytes, b_bytes,
        "bytes divergiram: v1={:?} v2={:?}",
        String::from_utf8_lossy(&a_bytes),
        String::from_utf8_lossy(&b_bytes)
    );
}

#[test]
fn append_jsonl_preserves_key_order_like_v1() {
    // Node preserva ordem de inserção em objeto. Rust precisa de
    // preserve_order no serde_json — ligado no Cargo.toml.
    if !node_available() {
        return;
    }
    let a_dir = TempDir::new().unwrap();
    let b_dir = TempDir::new().unwrap();
    let a = a_dir.path().join("ord.jsonl");
    let b = b_dir.path().join("ord.jsonl");
    let payload = r#"{"z":1,"a":2,"m":3}"#;

    call_v1("appendJsonl", &a, payload);
    append_jsonl(&b, &serde_json::from_str(payload).unwrap()).unwrap();

    let a_text = std::fs::read_to_string(&a).unwrap();
    let b_text = std::fs::read_to_string(&b).unwrap();
    assert_eq!(a_text, b_text);
    // Ambos devem conter "z":1 antes de "a":2.
    let z_pos = a_text.find("\"z\"").unwrap();
    let a_pos = a_text.find("\"a\"").unwrap();
    assert!(z_pos < a_pos);
}

#[test]
fn append_jsonl_creates_file_when_missing() {
    if !node_available() {
        return;
    }
    let a_dir = TempDir::new().unwrap();
    let b_dir = TempDir::new().unwrap();
    let a = a_dir.path().join("new.jsonl");
    let b = b_dir.path().join("new.jsonl");
    assert!(!a.exists() && !b.exists());
    let payload = r#"{"ok":true}"#;

    call_v1("appendJsonl", &a, payload);
    append_jsonl(&b, &serde_json::from_str(payload).unwrap()).unwrap();

    assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
}

// ─── readLastJsonlLine ─────────────────────────────────────────────────────

#[test]
fn read_last_none_when_missing_matches_v1() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let p = tmp.path().join("absent.jsonl");
    let v1 = call_v1("readLastJsonlLine", &p, "");
    assert_eq!(v1, "NULL");
    assert!(read_last_jsonl_line(&p).unwrap().is_none());
}

#[test]
fn read_last_none_when_empty_matches_v1() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let p = tmp.path().join("empty.jsonl");
    std::fs::write(&p, "").unwrap();
    let v1 = call_v1("readLastJsonlLine", &p, "");
    assert_eq!(v1, "NULL");
    assert!(read_last_jsonl_line(&p).unwrap().is_none());
}

#[test]
fn read_last_returns_last_valid_matches_v1() {
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let p = tmp.path().join("f.jsonl");
    std::fs::write(&p, "{\"n\":1}\n{\"n\":2}\n{\"n\":3}\n").unwrap();
    let v1 = call_v1("readLastJsonlLine", &p, "");
    let v2 = read_last_jsonl_line(&p).unwrap().unwrap();
    assert_eq!(v1, serde_json::to_string(&v2).unwrap());
}

#[test]
fn read_last_corrupted_tail_single_chunk_matches_v1() {
    // Arquivo cabe num chunk, última linha inválida → v1 devolve null.
    // Paridade exige o mesmo em v2.
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let p = tmp.path().join("bad.jsonl");
    std::fs::write(&p, "{\"n\":1}\n{\"n\":2}\nnotjson\n").unwrap();
    let v1 = call_v1("readLastJsonlLine", &p, "");
    assert_eq!(v1, "NULL");
    assert!(read_last_jsonl_line(&p).unwrap().is_none());
}

#[test]
fn read_last_line_bigger_than_chunk_matches_v1() {
    // Linha maior que 2KB exige múltiplos chunks para reconstruir.
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let p = tmp.path().join("big.jsonl");
    let big = "x".repeat(2048 * 3);
    let line = format!("{{\"big\":\"{big}\"}}\n");
    std::fs::write(&p, line).unwrap();
    let v1 = call_v1("readLastJsonlLine", &p, "");
    let v2 = read_last_jsonl_line(&p).unwrap().unwrap();
    assert_eq!(v1, serde_json::to_string(&v2).unwrap());
}

// ─── Auditoria: cenários que os parity anteriores não cobrem ─────────────

#[test]
fn audit_append_unicode_emoji_matches_v1() {
    if !node_available() {
        return;
    }
    let a_dir = TempDir::new().unwrap();
    let b_dir = TempDir::new().unwrap();
    let a = a_dir.path().join("u.jsonl");
    let b = b_dir.path().join("u.jsonl");
    let payload = r#"{"emoji":"café ☕","nome":"José"}"#;
    call_v1("appendJsonl", &a, payload);
    append_jsonl(&b, &serde_json::from_str(payload).unwrap()).unwrap();
    assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
}

#[test]
fn audit_append_special_chars_matches_v1() {
    if !node_available() {
        return;
    }
    let a_dir = TempDir::new().unwrap();
    let b_dir = TempDir::new().unwrap();
    let a = a_dir.path().join("s.jsonl");
    let b = b_dir.path().join("s.jsonl");
    // \n, \" precisam escapar igualmente.
    let payload = r#"{"nl":"line1\nline2","q":"with \"quotes\""}"#;
    call_v1("appendJsonl", &a, payload);
    append_jsonl(&b, &serde_json::from_str(payload).unwrap()).unwrap();
    assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
}

#[test]
fn audit_append_nested_preserves_order_matches_v1() {
    if !node_available() {
        return;
    }
    let a_dir = TempDir::new().unwrap();
    let b_dir = TempDir::new().unwrap();
    let a = a_dir.path().join("n.jsonl");
    let b = b_dir.path().join("n.jsonl");
    let payload = r#"{"outer":{"z":1,"a":2},"other":[{"b":1,"a":2}]}"#;
    call_v1("appendJsonl", &a, payload);
    append_jsonl(&b, &serde_json::from_str(payload).unwrap()).unwrap();
    assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
}

#[test]
fn audit_append_without_trailing_newline_produces_glued_bytes() {
    // Bug-feature do .cjs: arquivo sem \n final + appendJsonl
    // concatena sem separador. Paridade exige replicar.
    if !node_available() {
        return;
    }
    let a_dir = TempDir::new().unwrap();
    let b_dir = TempDir::new().unwrap();
    let a = a_dir.path().join("g.jsonl");
    let b = b_dir.path().join("g.jsonl");
    std::fs::write(&a, r#"{"x":1}"#).unwrap();
    std::fs::write(&b, r#"{"x":1}"#).unwrap();
    let payload = r#"{"y":2}"#;
    call_v1("appendJsonl", &a, payload);
    append_jsonl(&b, &serde_json::from_str(payload).unwrap()).unwrap();
    let a_bytes = std::fs::read(&a).unwrap();
    let b_bytes = std::fs::read(&b).unwrap();
    assert_eq!(a_bytes, b_bytes);
    assert_eq!(a_bytes, b"{\"x\":1}{\"y\":2}\n");
}

#[test]
#[ignore = "divergência conhecida: Node double arredonda int > 2^53; Rust i64 preserva. \
            Decidido não corrigir — produção usa UUIDs (string) e timestamps (cabem em i64). \
            Ver doc do módulo jsonl."]
fn audit_append_big_number_matches_v1() {
    // Número > 2^53 — Node perde precisão (JS só tem double).
    // serde_json preserva i64 até i64::MAX.
    if !node_available() {
        return;
    }
    let a_dir = TempDir::new().unwrap();
    let b_dir = TempDir::new().unwrap();
    let a = a_dir.path().join("bn.jsonl");
    let b = b_dir.path().join("bn.jsonl");
    let payload = r#"{"big":9007199254740993}"#;
    call_v1("appendJsonl", &a, payload);
    append_jsonl(&b, &serde_json::from_str(payload).unwrap()).unwrap();
    let a_bytes = std::fs::read(&a).unwrap();
    let b_bytes = std::fs::read(&b).unwrap();
    assert_eq!(
        a_bytes, b_bytes,
        "big int divergiu (comportamento esperado?): v1={:?} v2={:?}",
        String::from_utf8_lossy(&a_bytes),
        String::from_utf8_lossy(&b_bytes)
    );
}

#[test]
fn audit_append_float_scientific_matches_v1() {
    // Notação científica: Node emite `1e+30`. serde_json pode emitir
    // diferente.
    if !node_available() {
        return;
    }
    let a_dir = TempDir::new().unwrap();
    let b_dir = TempDir::new().unwrap();
    let a = a_dir.path().join("fl.jsonl");
    let b = b_dir.path().join("fl.jsonl");
    let payload = r#"{"pi":3.141592653589793,"sci":1e30}"#;
    call_v1("appendJsonl", &a, payload);
    append_jsonl(&b, &serde_json::from_str(payload).unwrap()).unwrap();
    let a_bytes = std::fs::read(&a).unwrap();
    let b_bytes = std::fs::read(&b).unwrap();
    assert_eq!(
        a_bytes, b_bytes,
        "float divergiu: v1={:?} v2={:?}",
        String::from_utf8_lossy(&a_bytes),
        String::from_utf8_lossy(&b_bytes)
    );
}

#[test]
fn audit_read_jsonl_with_utf8_bom_matches_v1() {
    // BOM na frente: .cjs não trata, readFile devolve string com BOM
    // embutido, split('\n') mantém, JSON.parse falha na primeira linha
    // → descartada. Resultado: [].
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let p = tmp.path().join("bom.jsonl");
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(b"{\"a\":1}\n");
    std::fs::write(&p, bytes).unwrap();
    let v1 = call_v1("readJsonl", &p, "");
    let v2 = serde_json::to_string(&read_jsonl(&p).unwrap()).unwrap();
    assert_eq!(v1, v2);
}

#[test]
fn read_last_many_small_lines_crossing_chunk_matches_v1() {
    // Muitas linhas curtas cobrem vários chunks — testa que read_jsonl
    // backward concatena certo sem perder caracteres no boundary.
    if !node_available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let p = tmp.path().join("many.jsonl");
    let mut buf = String::new();
    for i in 0..1000 {
        buf.push_str(&format!("{{\"i\":{i}}}\n"));
    }
    std::fs::write(&p, buf).unwrap();
    let v1 = call_v1("readLastJsonlLine", &p, "");
    let v2 = read_last_jsonl_line(&p).unwrap().unwrap();
    assert_eq!(v1, serde_json::to_string(&v2).unwrap());
}
