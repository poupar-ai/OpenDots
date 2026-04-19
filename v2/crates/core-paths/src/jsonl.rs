//! I/O de arquivos JSONL (JSON-por-linha).
//!
//! Porta literal de `readJsonl`, `appendJsonl` e `readLastJsonlLine` do
//! `plugin/scripts/lib.cjs`. Usado por `observations.jsonl`,
//! `sessions.jsonl`, `messages/YYYY-MM-DD.jsonl`, `agent_chats/*.jsonl`,
//! `human_logs/*.jsonl` e outros arquivos append-only do project-mem.
//!
//! ## Decisões de paridade não-óbvias
//!
//! - **Linhas que parseiam para `null`/`0`/`false`/`""` somem.** O `.cjs`
//!   faz `.filter(Boolean)` — efetivamente dropa qualquer valor falsy
//!   pós-parse. Replicado em [`is_truthy`].
//! - **Parse inválido é silencioso.** Linhas que falham `JSON.parse` são
//!   removidas sem log. Mesmo no Rust.
//! - **`read_last_jsonl_line` lê backward em chunks de 2KB.** Evita ler
//!   arquivos grandes só para pegar a última linha. **NÃO faz scan
//!   retroativo** pela penúltima linha se a última é corrompida — se
//!   o arquivo cabe num único chunk e a última linha falhou parse,
//!   devolve `None`. Validado contra o `.cjs` em parity test.
//! - **Preservação de ordem de chaves.** `JSON.stringify` do Node
//!   preserva a ordem em que as chaves foram inseridas no objeto. Em
//!   Rust precisamos da feature `serde_json/preserve_order` para o
//!   mesmo comportamento — habilitada no `Cargo.toml` deste crate.
//! - **Append sem `\n` no final concatena colado.** Se o arquivo
//!   existente não termina em `\n`, o próximo `append_jsonl` vira
//!   `}{` grudado na linha anterior. Bug-feature do `.cjs`,
//!   replicado literalmente.
//!
//! ## Divergência conhecida (não corrigida)
//!
//! **Inteiros `> 2^53` perdem precisão em Node, Rust preserva.**
//! Ex.: `{"big":9007199254740993}` vira `9007199254740992` no `.cjs`
//! (limitação do double de 64 bits do V8) e fica preservado em Rust.
//! Não corrigimos porque: (a) produção nunca usa inteiros tão grandes
//! — IDs são UUIDs (string), timestamps cabem em i64 até o ano 294276;
//! (b) replicar a perda exigiria walk recursivo em cada append + float
//! em vez de int, herdando o problema de formato de f64.
//! Ver `parity_jsonl.rs::audit_append_big_number_matches_v1`
//! (marcado `#[ignore]` com razão documentada).

use serde_json::Value;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum JsonlError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Replica `readJsonl(file)` do `.cjs`:
/// - Se o arquivo não existe, retorna `Ok(vec![])`.
/// - Split por `\n`.
/// - Linhas que `trim().is_empty()` são filtradas.
/// - Cada linha vira `Value` via `serde_json::from_str`; erros de parse
///   viram `null`.
/// - Valores falsy (`null`, `false`, `0`, `""`) são descartados, igual
///   ao `filter(Boolean)` do JS.
///
/// Erros de I/O (permissão, etc.) propagam via `JsonlError::Io`.
pub fn read_jsonl(path: &Path) -> Result<Vec<Value>, JsonlError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(path)?;
    let out = content
        .split('\n')
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(is_truthy)
        .collect();
    Ok(out)
}

/// Replica `appendJsonl(file, obj)`:
/// grava `JSON.stringify(obj) + '\n'` em append mode. Cria o arquivo
/// se não existir.
///
/// Não cria diretórios pai — o caller é responsável (igual ao `.cjs`,
/// que também lança se o diretório não existe).
pub fn append_jsonl(path: &Path, value: &Value) -> Result<(), JsonlError> {
    let line = serde_json::to_string(value)
        .expect("Value::to_string não falha para Value válido");
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(line.as_bytes())?;
    f.write_all(b"\n")?;
    Ok(())
}

/// Tamanho do chunk de leitura backward. Igual ao `.cjs` para manter
/// comportamento idêntico em linhas que cruzam boundary.
const CHUNK_SIZE: usize = 2048;

/// Replica `readLastJsonlLine(file)`:
/// - Se o arquivo não existe, retorna `Ok(None)`.
/// - Se tem tamanho 0, retorna `Ok(None)`.
/// - Caso contrário, lê do fim em chunks de 2KB acumulando um buffer
///   crescente por trás; sempre que o buffer tem pelo menos uma linha
///   não-vazia, tenta parsear **a última**. Se o parse falhar, prossegue
///   para o próximo chunk — a intenção é lidar com linha grande que
///   cruza o boundary (chunk atual tem só o sufixo da linha, próximo
///   completa).
/// - **Se chegar ao início do arquivo sem uma última linha parseável,
///   retorna `Ok(None)`** — não faz scan retroativo pela penúltima.
///   Esse é o mesmo comportamento do `.cjs`: uma última linha corrompida
///   em arquivo que cabe num chunk faz a função devolver null.
pub fn read_last_jsonl_line(path: &Path) -> Result<Option<Value>, JsonlError> {
    if !path.exists() {
        return Ok(None);
    }
    let mut f = File::open(path)?;
    let total = f.metadata()?.len();
    if total == 0 {
        return Ok(None);
    }

    let mut pos: u64 = total;
    let mut buf = String::new();

    while pos > 0 {
        let read_size = std::cmp::min(CHUNK_SIZE as u64, pos) as usize;
        pos -= read_size as u64;
        f.seek(SeekFrom::Start(pos))?;
        let mut chunk = vec![0u8; read_size];
        f.read_exact(&mut chunk)?;
        // Prepende chunk ao buffer acumulado. `String::from_utf8_lossy`
        // lida com caso de chunk começar no meio de um code point UTF-8
        // — quando completar o caractere na próxima iteração, a porção
        // do meio passa a decodificar corretamente. (O `.cjs` faz
        // `chunk.toString('utf8')` que também substitui bytes inválidos.)
        let chunk_str = String::from_utf8_lossy(&chunk).into_owned();
        buf = chunk_str + &buf;

        let lines: Vec<&str> = buf
            .split('\n')
            .filter(|l| !l.trim().is_empty())
            .collect();
        if let Some(last) = lines.last() {
            if let Ok(v) = serde_json::from_str::<Value>(last) {
                return Ok(Some(v));
            }
            // Parse falhou — pode ser linha quebrada no chunk boundary.
            // Continua subindo para completar.
        }
    }
    Ok(None)
}

/// Equivalente a `Boolean(v)` do JS aplicado a um `Value`:
/// `null`, `false`, `0` (e variantes), `""` são falsy.
fn is_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => {
            // `Boolean(0) === false`, `Boolean(NaN) === false`.
            // Em serde_json::Number, NaN/Infinity não existem (parse falha),
            // então basta checar zero (i64/u64/f64).
            if let Some(i) = n.as_i64() {
                i != 0
            } else if let Some(u) = n.as_u64() {
                u != 0
            } else if let Some(f) = n.as_f64() {
                f != 0.0 && !f.is_nan()
            } else {
                true
            }
        }
        Value::String(s) => !s.is_empty(),
        // Object e Array sempre são truthy em JS (`Boolean({}) === true`).
        Value::Object(_) | Value::Array(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn read_jsonl_returns_empty_when_missing() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("noexist.jsonl");
        assert!(read_jsonl(&p).unwrap().is_empty());
    }

    #[test]
    fn append_and_read_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("obs.jsonl");
        append_jsonl(&p, &serde_json::json!({ "a": 1 })).unwrap();
        append_jsonl(&p, &serde_json::json!({ "b": 2 })).unwrap();
        let all = read_jsonl(&p).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0]["a"], 1);
        assert_eq!(all[1]["b"], 2);
    }

    #[test]
    fn read_jsonl_skips_blank_and_invalid_lines() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("mixed.jsonl");
        std::fs::write(&p, "{\"a\":1}\n\n  \nnope\n{\"b\":2}\n").unwrap();
        let all = read_jsonl(&p).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0]["a"], 1);
        assert_eq!(all[1]["b"], 2);
    }

    #[test]
    fn read_jsonl_drops_falsy_values_like_js() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("falsy.jsonl");
        std::fs::write(&p, "{\"x\":1}\nnull\nfalse\n0\n\"\"\n{\"y\":2}\n").unwrap();
        let all = read_jsonl(&p).unwrap();
        // Só sobrevivem {x:1} e {y:2}.
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn read_last_jsonl_line_none_when_missing() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("absent.jsonl");
        assert!(read_last_jsonl_line(&p).unwrap().is_none());
    }

    #[test]
    fn read_last_jsonl_line_none_when_empty() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("empty.jsonl");
        std::fs::write(&p, "").unwrap();
        assert!(read_last_jsonl_line(&p).unwrap().is_none());
    }

    #[test]
    fn read_last_jsonl_line_returns_last_valid() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("last.jsonl");
        append_jsonl(&p, &serde_json::json!({ "n": 1 })).unwrap();
        append_jsonl(&p, &serde_json::json!({ "n": 2 })).unwrap();
        append_jsonl(&p, &serde_json::json!({ "n": 3 })).unwrap();
        let got = read_last_jsonl_line(&p).unwrap().unwrap();
        assert_eq!(got["n"], 3);
    }

    #[test]
    fn read_last_jsonl_line_returns_none_when_tail_corrupted_and_file_fits_chunk() {
        // Confirmado empiricamente contra o .cjs: arquivo com última
        // linha inválida, pequeno o suficiente pra caber num chunk de
        // 2KB, devolve null (não faz scan retroativo pela penúltima).
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("corrupt.jsonl");
        std::fs::write(&p, "{\"n\":1}\n{\"n\":2}\nnotjson\n").unwrap();
        assert!(read_last_jsonl_line(&p).unwrap().is_none());
    }

    #[test]
    fn read_last_jsonl_line_handles_line_longer_than_chunk() {
        // Linha maior que CHUNK_SIZE força múltiplas iterações para
        // reconstruir antes de parsear.
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("big.jsonl");
        let big_val = "x".repeat(CHUNK_SIZE * 3);
        let line = serde_json::json!({ "big": big_val });
        append_jsonl(&p, &line).unwrap();
        let got = read_last_jsonl_line(&p).unwrap().unwrap();
        assert_eq!(got["big"].as_str().unwrap().len(), CHUNK_SIZE * 3);
    }

    #[test]
    fn append_preserves_key_order_for_parity() {
        // JSON.stringify do Node preserva ordem de inserção.
        // Em Rust precisa da feature preserve_order.
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("order.jsonl");
        let mut m = serde_json::Map::new();
        m.insert("z".to_string(), serde_json::json!(1));
        m.insert("a".to_string(), serde_json::json!(2));
        append_jsonl(&p, &Value::Object(m)).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert_eq!(text, "{\"z\":1,\"a\":2}\n");
    }
}
