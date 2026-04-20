//! Paridade v1 (`plugin/scripts/lib.cjs :: readStdin / getCwd`) vs v2
//! (`core_paths::stdin::{read_stdin_from_str, get_cwd_from}`).
//!
//! Estratégia:
//!
//! - `readStdin`: spawn node com stdin injetado via `ChildStdin::write_all`,
//!   capturar `JSON.stringify(readStdin())` no stdout. Comparar com
//!   `serde_json::to_string` do resultado Rust.
//! - `getCwd`: idem, mas também injetando env vars no processo filho.
//!
//! Para `readStdin` sem TTY check: node herda o pipe stdin do pai,
//! então `process.stdin.isTTY` é `undefined` (falsy). O path que toca
//! `isTTY` não é exercitado — documentamos que a saída TTY do Rust
//! também devolve `{}`, mas não testamos via spawn porque injetar TTY
//! em subprocess exige pty (openpty) e foge do escopo.

use core_paths::{read_stdin_from_str, stdin as core_stdin};
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const NODE_SCRIPT_READ: &str = r#"
const lib = require(process.argv[1]);
const r = lib.readStdin();
process.stdout.write(JSON.stringify(r));
"#;

const NODE_SCRIPT_GETCWD: &str = r#"
const lib = require(process.argv[1]);
const input = lib.readStdin();
const r = lib.getCwd(input);
process.stdout.write(String(r));
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

/// Spawn node with a script, pipe `stdin_bytes` into stdin, return stdout as string.
fn call_v1_read_stdin(stdin_bytes: &[u8]) -> String {
    let mut child = Command::new("node")
        .arg("-e")
        .arg(NODE_SCRIPT_READ)
        .arg(lib_cjs_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn node");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin_bytes)
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    assert!(
        out.status.success(),
        "node falhou: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf8")
}

fn call_v1_get_cwd(stdin_bytes: &[u8], env: &[(&str, &str)]) -> String {
    let mut cmd = Command::new("node");
    cmd.arg("-e")
        .arg(NODE_SCRIPT_GETCWD)
        .arg(lib_cjs_path())
        .env_clear()
        // Preservar PATH para achar o node (mesmo com env_clear o node binário já foi resolvido,
        // mas child_process pode precisar). PATH herdado de `std::env::var`.
        .env(
            "PATH",
            std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().expect("spawn node");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin_bytes)
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    assert!(
        out.status.success(),
        "node falhou: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf8")
}

/// Paridade byte-a-byte via `JSON.stringify` (JS) vs `serde_json::to_string` (Rust).
/// `preserve_order` está ativado no serde_json, e objetos usam IndexMap
/// mantendo insertion order — mesmo comportamento de `JSON.stringify`
/// sobre Objects.
fn stringify_value(v: &Value) -> String {
    serde_json::to_string(v).expect("serde stringify")
}

// ─── readStdin — paridade ──────────────────────────────────────────────────

#[test]
fn parity_read_stdin_empty_object() {
    if !node_available() {
        return;
    }
    let v1 = call_v1_read_stdin(b"{}");
    let v2 = stringify_value(&read_stdin_from_str("{}"));
    assert_eq!(v1, v2);
}

#[test]
fn parity_read_stdin_object_with_cwd() {
    if !node_available() {
        return;
    }
    let s = r#"{"cwd":"/tmp","trace_id":"abc"}"#;
    let v1 = call_v1_read_stdin(s.as_bytes());
    let v2 = stringify_value(&read_stdin_from_str(s));
    assert_eq!(v1, v2);
}

#[test]
fn parity_read_stdin_empty_input_returns_empty_obj() {
    if !node_available() {
        return;
    }
    // Nada no stdin — JSON.parse("") throws → {}. Rust read_stdin_from_str("") → {}.
    let v1 = call_v1_read_stdin(b"");
    let v2 = stringify_value(&read_stdin_from_str(""));
    assert_eq!(v1, "{}");
    assert_eq!(v2, "{}");
}

#[test]
fn parity_read_stdin_malformed_returns_empty_obj() {
    if !node_available() {
        return;
    }
    let s = "{not json";
    let v1 = call_v1_read_stdin(s.as_bytes());
    let v2 = stringify_value(&read_stdin_from_str(s));
    assert_eq!(v1, "{}");
    assert_eq!(v2, "{}");
}

#[test]
fn parity_read_stdin_array() {
    if !node_available() {
        return;
    }
    let s = "[1,2,3]";
    let v1 = call_v1_read_stdin(s.as_bytes());
    let v2 = stringify_value(&read_stdin_from_str(s));
    assert_eq!(v1, v2);
}

#[test]
fn parity_read_stdin_null() {
    if !node_available() {
        return;
    }
    let v1 = call_v1_read_stdin(b"null");
    let v2 = stringify_value(&read_stdin_from_str("null"));
    assert_eq!(v1, "null");
    assert_eq!(v2, "null");
}

#[test]
fn parity_read_stdin_nested_object_key_order() {
    if !node_available() {
        return;
    }
    // Ordem de chaves do JSON original deve ser preservada em ambos.
    let s = r#"{"z":1,"a":2,"m":3}"#;
    let v1 = call_v1_read_stdin(s.as_bytes());
    let v2 = stringify_value(&read_stdin_from_str(s));
    assert_eq!(v1, v2);
    assert_eq!(v1, s);
}

#[test]
fn parity_read_stdin_unicode_content() {
    if !node_available() {
        return;
    }
    let s = r#"{"msg":"olá éàç 🦀"}"#;
    let v1 = call_v1_read_stdin(s.as_bytes());
    let v2 = stringify_value(&read_stdin_from_str(s));
    assert_eq!(v1, v2);
}

// ─── getCwd — paridade ─────────────────────────────────────────────────────

#[test]
fn parity_getcwd_from_input_string() {
    if !node_available() {
        return;
    }
    let stdin = r#"{"cwd":"/from-input"}"#;
    let env = [("CLAUDE_CWD", "/ignored"), ("PWD", "/also-ignored")];
    let v1 = call_v1_get_cwd(stdin.as_bytes(), &env);
    let v2 = core_stdin::get_cwd_from(
        Some(&read_stdin_from_str(stdin)),
        Some("/ignored"),
        Some("/also-ignored"),
        || PathBuf::from("/fallback"),
    )
    .to_string_lossy()
    .to_string();
    assert_eq!(v1, v2);
}

#[test]
fn parity_getcwd_skips_empty_input_cwd() {
    if !node_available() {
        return;
    }
    let stdin = r#"{"cwd":""}"#;
    let env = [("CLAUDE_CWD", "/c"), ("PWD", "/p")];
    let v1 = call_v1_get_cwd(stdin.as_bytes(), &env);
    let v2 = core_stdin::get_cwd_from(
        Some(&read_stdin_from_str(stdin)),
        Some("/c"),
        Some("/p"),
        || PathBuf::from("/fb"),
    )
    .to_string_lossy()
    .to_string();
    assert_eq!(v1, v2);
}

#[test]
fn parity_getcwd_skips_missing_input_cwd() {
    if !node_available() {
        return;
    }
    let stdin = r#"{"trace_id":"abc"}"#;
    let env = [("CLAUDE_CWD", "/c"), ("PWD", "/p")];
    let v1 = call_v1_get_cwd(stdin.as_bytes(), &env);
    let v2 = core_stdin::get_cwd_from(
        Some(&read_stdin_from_str(stdin)),
        Some("/c"),
        Some("/p"),
        || PathBuf::from("/fb"),
    )
    .to_string_lossy()
    .to_string();
    assert_eq!(v1, v2);
}

#[test]
fn parity_getcwd_from_claude_cwd_env() {
    if !node_available() {
        return;
    }
    // stdin vazio → readStdin retorna {} → input.cwd undefined → CLAUDE_CWD.
    let env = [("CLAUDE_CWD", "/claude"), ("PWD", "/pwd")];
    let v1 = call_v1_get_cwd(b"", &env);
    let v2 = core_stdin::get_cwd_from(
        Some(&read_stdin_from_str("")),
        Some("/claude"),
        Some("/pwd"),
        || PathBuf::from("/fb"),
    )
    .to_string_lossy()
    .to_string();
    assert_eq!(v1, v2);
}

#[test]
fn parity_getcwd_falls_to_pwd() {
    if !node_available() {
        return;
    }
    // CLAUDE_CWD ausente, PWD presente.
    let env = [("PWD", "/pwd")];
    let v1 = call_v1_get_cwd(b"{}", &env);
    let v2 = core_stdin::get_cwd_from(
        Some(&read_stdin_from_str("{}")),
        None,
        Some("/pwd"),
        || PathBuf::from("/fb"),
    )
    .to_string_lossy()
    .to_string();
    assert_eq!(v1, v2);
}

#[test]
fn parity_getcwd_empty_claude_env_falls_to_pwd() {
    if !node_available() {
        return;
    }
    // CLAUDE_CWD = "" é falsy em JS → cai pra PWD.
    let env = [("CLAUDE_CWD", ""), ("PWD", "/pwd")];
    let v1 = call_v1_get_cwd(b"{}", &env);
    let v2 = core_stdin::get_cwd_from(
        Some(&read_stdin_from_str("{}")),
        Some(""),
        Some("/pwd"),
        || PathBuf::from("/fb"),
    )
    .to_string_lossy()
    .to_string();
    assert_eq!(v1, v2);
}

#[test]
fn parity_getcwd_all_env_missing_uses_fallback() {
    if !node_available() {
        return;
    }
    // Sem CLAUDE_CWD nem PWD → process.cwd(). O node subprocess herda
    // o cwd do processo pai (o test runner). Para paridade, Rust usa
    // o mesmo current_dir via closure.
    let env: [(&str, &str); 0] = [];
    let v1 = call_v1_get_cwd(b"{}", &env);
    let v2 = core_stdin::get_cwd_from(
        Some(&read_stdin_from_str("{}")),
        None,
        None,
        || std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    )
    .to_string_lossy()
    .to_string();
    assert_eq!(v1, v2);
}

#[test]
fn parity_getcwd_non_object_input_falls_through() {
    if !node_available() {
        return;
    }
    // stdin = "null" → JSON.parse = null → input = null → (null && null.cwd) = null → falsy.
    // Rust readStdin retorna Value::Null → get_cwd_from não tem Value::Object → cai.
    let stdin = "null";
    let env = [("CLAUDE_CWD", "/c")];
    let v1 = call_v1_get_cwd(stdin.as_bytes(), &env);
    let v2 = core_stdin::get_cwd_from(
        Some(&read_stdin_from_str(stdin)),
        Some("/c"),
        None,
        || PathBuf::from("/fb"),
    )
    .to_string_lossy()
    .to_string();
    assert_eq!(v1, v2);
}

#[test]
fn parity_getcwd_array_input_has_no_cwd() {
    if !node_available() {
        return;
    }
    // Array: `arr.cwd` é `undefined` → falsy → cai.
    let stdin = "[1,2]";
    let env = [("CLAUDE_CWD", "/c")];
    let v1 = call_v1_get_cwd(stdin.as_bytes(), &env);
    let v2 = core_stdin::get_cwd_from(
        Some(&read_stdin_from_str(stdin)),
        Some("/c"),
        None,
        || PathBuf::from("/fb"),
    )
    .to_string_lossy()
    .to_string();
    assert_eq!(v1, v2);
}

#[test]
fn parity_getcwd_cwd_with_spaces_and_unicode() {
    if !node_available() {
        return;
    }
    let stdin = r#"{"cwd":"/home/usuário/with space/pasta"}"#;
    let env = [("CLAUDE_CWD", "/c")];
    let v1 = call_v1_get_cwd(stdin.as_bytes(), &env);
    let v2 = core_stdin::get_cwd_from(
        Some(&read_stdin_from_str(stdin)),
        Some("/c"),
        None,
        || PathBuf::from("/fb"),
    )
    .to_string_lossy()
    .to_string();
    assert_eq!(v1, v2);
}

#[test]
fn parity_getcwd_consciously_diverges_on_non_string_cwd() {
    // F2-20: `.cjs` aceita input.cwd não-string truthy (number, array)
    // e retorna como-é (JS depois coage implicitamente). Rust só aceita
    // string não-vazia — cai para env. Este teste documenta a
    // divergência consciente, NÃO verifica paridade byte-a-byte.
    if !node_available() {
        return;
    }
    let stdin = r#"{"cwd":42}"#;
    let env = [("CLAUDE_CWD", "/c")];
    let v1 = call_v1_get_cwd(stdin.as_bytes(), &env);
    let v2 = core_stdin::get_cwd_from(
        Some(&read_stdin_from_str(stdin)),
        Some("/c"),
        None,
        || PathBuf::from("/fb"),
    )
    .to_string_lossy()
    .to_string();
    assert_eq!(v1, "42", ".cjs coerce number → string");
    assert_eq!(v2, "/c", "Rust consciously falls through");
    assert_ne!(v1, v2, "divergência consciente registrada como F2-20");
}

#[test]
fn parity_getcwd_empty_pwd_falls_to_current_dir() {
    if !node_available() {
        return;
    }
    let env = [("PWD", "")];
    let v1 = call_v1_get_cwd(b"{}", &env);
    let v2 = core_stdin::get_cwd_from(
        Some(&read_stdin_from_str("{}")),
        None,
        Some(""),
        || std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    )
    .to_string_lossy()
    .to_string();
    assert_eq!(v1, v2);
}

// ─── readStdin + getCwd integração ────────────────────────────────────────

#[test]
fn parity_sanity_json_with_extra_fields_unused_by_getcwd() {
    if !node_available() {
        return;
    }
    let stdin = json!({
        "cwd": "/x",
        "trace_id": "abc",
        "nested": {"a": 1}
    })
    .to_string();
    let env = [("CLAUDE_CWD", "/c")];
    let v1 = call_v1_get_cwd(stdin.as_bytes(), &env);
    let v2 = core_stdin::get_cwd_from(
        Some(&read_stdin_from_str(&stdin)),
        Some("/c"),
        None,
        || PathBuf::from("/fb"),
    )
    .to_string_lossy()
    .to_string();
    assert_eq!(v1, v2);
    assert_eq!(v1, "/x");
}
