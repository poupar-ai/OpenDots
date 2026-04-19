//! Paridade v1 (`plugin/scripts/lib.cjs :: estimateTokens`) vs v2
//! (`core_paths::estimate_tokens`).
//!
//! Spawn `node -e` carregando o `.cjs` real, roda `estimateTokens(input)` e
//! compara byte-a-byte com o retorno da função Rust. Passa o input via
//! stdin (UTF-8) para evitar qualquer serialização intermediária — alguns
//! caracteres (emoji suplementar, ZWJ) são sensíveis a escapes em argv.

use core_paths::estimate_tokens;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const NODE_SCRIPT: &str = r#"
const lib = require(process.argv[1]);
let data = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', c => data += c);
process.stdin.on('end', () => {
  const n = lib.estimateTokens(data);
  process.stdout.write(String(n));
});
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

fn call_v1(input: &str) -> usize {
    let mut child = Command::new("node")
        .arg("-e")
        .arg(NODE_SCRIPT)
        .arg(lib_cjs_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn node");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait node");
    assert!(
        out.status.success(),
        "node falhou: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    s.parse::<usize>()
        .unwrap_or_else(|_| panic!("v1 retornou não-número: {s:?}"))
}

fn assert_parity(label: &str, input: &str) {
    let v1 = call_v1(input);
    let v2 = estimate_tokens(input);
    assert_eq!(
        v1, v2,
        "diverge em {label}: v1={v1} v2={v2} input.len={}",
        input.len()
    );
}

#[test]
fn parity_empty_string() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    assert_parity("empty", "");
}

#[test]
fn parity_ascii_short() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    assert_parity("ascii short", "hello world");
    assert_parity("ascii 1 char", "a");
    assert_parity("ascii 7 chars", "abcdefg"); // ceil(7/3.5)=2
}

#[test]
fn parity_string_zero_digit() {
    // Caller pode passar "0" (string, não number). Em JS, `!"0"` é false
    // (string não-vazia é truthy), então não cai no early return.
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    assert_parity("0-string", "0");
}

#[test]
fn parity_portuguese_accents() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    assert_parity("café", "café");
    assert_parity("caféééééé", "caféééééé");
    assert_parity(
        "frase PT",
        "A raposa marrom pulou sobre o cão preguiçoso — 2026!",
    );
}

#[test]
fn parity_bmp_single_char() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    assert_parity("coffee cup", "☕");
    assert_parity("hiragana", "あ");
    assert_parity("cjk", "中文");
}

#[test]
fn parity_supplementary_emoji() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    assert_parity("grinning", "😀");
    assert_parity("rocket", "🚀");
    assert_parity("5 rockets", "🚀🚀🚀🚀🚀");
}

#[test]
fn parity_zwj_sequence() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    assert_parity("family", "👨‍👩‍👧");
    assert_parity("family 4", "👨‍👩‍👧‍👦");
}

#[test]
fn parity_mixed_text() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    assert_parity(
        "mixed PT+emoji+zwj",
        "Olá 👋, família 👨‍👩‍👧 em Brasília — café ☕ às 14h.",
    );
}

#[test]
fn parity_large_text() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    // Recap/rules típicos podem passar de alguns KB. Cobre o caso real do
    // caller `session-init.cjs:140` (blocos de contexto injetados na sessão).
    let big = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(500);
    assert_parity("large 28k", &big);
}

#[test]
fn parity_multiline_with_newlines() {
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    assert_parity("multiline", "linha1\nlinha2\nlinha3");
    assert_parity("trailing newline", "linha\n");
    assert_parity("only newlines", "\n\n\n");
}

#[test]
fn parity_only_whitespace() {
    // `!"   "` em JS é false (string não-vazia é truthy) — caller do
    // session-init passa `trimmed`, mas outros callers futuros podem
    // passar whitespace puro. Precisa replicar: não é zero.
    if !node_available() {
        eprintln!("node indisponível — pulando");
        return;
    }
    assert_parity("spaces", "   ");
    assert_parity("tabs", "\t\t");
}
