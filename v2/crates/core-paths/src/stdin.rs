//! Stdin/Cwd helpers — porta de `readStdin()` e `getCwd(input)` em
//! `lib.cjs:189-200`.
//!
//! ```js
//! function readStdin() {
//!   if (process.stdin.isTTY) return {};
//!   try {
//!     return JSON.parse(fs.readFileSync(0, 'utf8'));
//!   } catch {
//!     return {};
//!   }
//! }
//!
//! function getCwd(input) {
//!   return (input && input.cwd) || process.env.CLAUDE_CWD || process.env.PWD || process.cwd();
//! }
//! ```
//!
//! ## Por que existe
//!
//! O ponto de entrada de cada hook do Claude Code é um script que recebe
//! o contexto (cwd, trace_id, etc.) por stdin como JSON. `readStdin()`
//! é o leitor defensivo: em terminal interativo ou JSON malformado,
//! devolve objeto vazio em vez de quebrar. `getCwd(input)` resolve o
//! diretório do projeto tentando 4 fontes em ordem.
//!
//! ## Divergências conscientes com o `.cjs`
//!
//! - `readStdin` devolve `serde_json::Value`, não um `Object` fechado.
//!   O `.cjs` pode retornar `null`/`true`/`42` se o stdin for
//!   `"null"`/`"true"`/`"42"` (JSON escalar). Tipando como `Value`,
//!   preservamos a paridade sem inventar coerções.
//! - `getCwd` só considera `input.cwd` quando é **string não-vazia**.
//!   O `.cjs` aceita qualquer valor truthy (número, array) e deixa a
//!   coerção implícita para o caller. Registrado como F2-20 (conscious
//!   divergence) — callers Rust recebem `PathBuf` tipado em troca.

use serde_json::Value;
use std::io::{self, IsTerminal, Read};
use std::path::PathBuf;

/// Lê stdin e parseia como JSON. Em TTY, stdin não-lido, ou JSON
/// inválido, devolve `{}`.
///
/// Replica `readStdin()` do `.cjs`. Retorna `Value` porque `JSON.parse`
/// aceita qualquer valor JSON (array, string, number). A fallback
/// defensiva é sempre `Value::Object({})`.
pub fn read_stdin() -> Value {
    if io::stdin().is_terminal() {
        return empty_object();
    }
    let mut buf = String::new();
    if io::stdin().read_to_string(&mut buf).is_err() {
        return empty_object();
    }
    read_stdin_from_str(&buf)
}

/// Versão testável: parse de uma string JSON já lida. Útil para unit
/// tests sem mexer em `process::stdin`.
pub fn read_stdin_from_str(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| empty_object())
}

fn empty_object() -> Value {
    Value::Object(serde_json::Map::new())
}

/// Resolve o cwd do projeto em 4 fontes (ordem idêntica ao `.cjs`):
/// 1. `input.cwd` se for string não-vazia.
/// 2. Env `CLAUDE_CWD`.
/// 3. Env `PWD`.
/// 4. `std::env::current_dir()`; se falhar, `PathBuf::from(".")`.
///
/// A verificação "string não-vazia" replica o JS `(input && input.cwd) || ...`
/// para o caso realista em que `input.cwd` vem de um JSON parse.
pub fn get_cwd(input: Option<&Value>) -> PathBuf {
    let claude_cwd = std::env::var("CLAUDE_CWD").ok();
    let pwd = std::env::var("PWD").ok();
    get_cwd_from(
        input,
        claude_cwd.as_deref(),
        pwd.as_deref(),
        || std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    )
}

/// Core resolvable: injetável com env/cwd artificiais. Usado pelos
/// unit tests para não tocar em `std::env` global (que não é
/// thread-safe em `cargo test`). Público para permitir parity tests
/// externos ao crate — callers de produção devem usar [`get_cwd`].
pub fn get_cwd_from(
    input: Option<&Value>,
    claude_cwd: Option<&str>,
    pwd: Option<&str>,
    fallback: impl FnOnce() -> PathBuf,
) -> PathBuf {
    // Caminho 1: input.cwd string não-vazia.
    if let Some(Value::Object(map)) = input {
        if let Some(Value::String(s)) = map.get("cwd") {
            if !s.is_empty() {
                return PathBuf::from(s);
            }
        }
    }
    // Caminho 2: CLAUDE_CWD.
    if let Some(s) = claude_cwd {
        if !s.is_empty() {
            return PathBuf::from(s);
        }
    }
    // Caminho 3: PWD.
    if let Some(s) = pwd {
        if !s.is_empty() {
            return PathBuf::from(s);
        }
    }
    // Caminho 4: current_dir.
    fallback()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ─── read_stdin_from_str ────────────────────────────────────────────────

    #[test]
    fn parse_object_returns_object() {
        let v = read_stdin_from_str(r#"{"cwd":"/tmp","x":1}"#);
        assert_eq!(v["cwd"], json!("/tmp"));
        assert_eq!(v["x"], json!(1));
    }

    #[test]
    fn parse_empty_returns_empty_object() {
        let v = read_stdin_from_str("");
        assert_eq!(v, json!({}));
    }

    #[test]
    fn parse_malformed_returns_empty_object() {
        let v = read_stdin_from_str("{not json");
        assert_eq!(v, json!({}));
    }

    #[test]
    fn parse_array_returns_array() {
        // JSON.parse("[1,2]") === [1,2], readStdin returns it as-is.
        let v = read_stdin_from_str("[1,2,3]");
        assert_eq!(v, json!([1, 2, 3]));
    }

    #[test]
    fn parse_null_returns_null() {
        let v = read_stdin_from_str("null");
        assert_eq!(v, Value::Null);
    }

    #[test]
    fn parse_scalar_returns_scalar() {
        assert_eq!(read_stdin_from_str("42"), json!(42));
        assert_eq!(read_stdin_from_str(r#""hi""#), json!("hi"));
        assert_eq!(read_stdin_from_str("true"), json!(true));
    }

    // ─── get_cwd_from ────────────────────────────────────────────────────────

    fn fb() -> PathBuf {
        PathBuf::from("/fallback")
    }

    #[test]
    fn cwd_from_input_when_non_empty_string() {
        let input = json!({"cwd": "/from-input"});
        let got = get_cwd_from(Some(&input), Some("/env-claude"), Some("/env-pwd"), fb);
        assert_eq!(got, PathBuf::from("/from-input"));
    }

    #[test]
    fn cwd_skips_input_when_empty_string() {
        // input.cwd = "" é falsy em JS; cai para CLAUDE_CWD.
        let input = json!({"cwd": ""});
        let got = get_cwd_from(Some(&input), Some("/env-claude"), Some("/env-pwd"), fb);
        assert_eq!(got, PathBuf::from("/env-claude"));
    }

    #[test]
    fn cwd_skips_input_when_cwd_missing() {
        let input = json!({"other": "x"});
        let got = get_cwd_from(Some(&input), Some("/env-claude"), None, fb);
        assert_eq!(got, PathBuf::from("/env-claude"));
    }

    #[test]
    fn cwd_skips_non_string_input_cwd() {
        // Divergência consciente (F2-20): input.cwd não-string → cai.
        let input = json!({"cwd": 42});
        let got = get_cwd_from(Some(&input), Some("/env-claude"), None, fb);
        assert_eq!(got, PathBuf::from("/env-claude"));
    }

    #[test]
    fn cwd_from_claude_env_when_input_missing() {
        let got = get_cwd_from(None, Some("/env-claude"), Some("/env-pwd"), fb);
        assert_eq!(got, PathBuf::from("/env-claude"));
    }

    #[test]
    fn cwd_skips_empty_claude_env() {
        let got = get_cwd_from(None, Some(""), Some("/env-pwd"), fb);
        assert_eq!(got, PathBuf::from("/env-pwd"));
    }

    #[test]
    fn cwd_from_pwd_when_claude_missing() {
        let got = get_cwd_from(None, None, Some("/env-pwd"), fb);
        assert_eq!(got, PathBuf::from("/env-pwd"));
    }

    #[test]
    fn cwd_skips_empty_pwd() {
        let got = get_cwd_from(None, None, Some(""), fb);
        assert_eq!(got, PathBuf::from("/fallback"));
    }

    #[test]
    fn cwd_fallback_when_all_missing() {
        let got = get_cwd_from(None, None, None, fb);
        assert_eq!(got, PathBuf::from("/fallback"));
    }

    #[test]
    fn cwd_null_input_falls_through() {
        let input = Value::Null;
        let got = get_cwd_from(Some(&input), Some("/env-claude"), None, fb);
        assert_eq!(got, PathBuf::from("/env-claude"));
    }

    #[test]
    fn cwd_non_object_input_falls_through() {
        // input como string/array/number/bool não tem `cwd` field acessível.
        let input = json!("string");
        let got = get_cwd_from(Some(&input), Some("/env-claude"), None, fb);
        assert_eq!(got, PathBuf::from("/env-claude"));
    }
}
