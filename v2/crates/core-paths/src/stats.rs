//! Stats cache — porta de `loadStats` / `saveStats` / `incrementStats` em
//! `lib.cjs`.
//!
//! ```js
//! function loadStats(memDir) {
//!   const f = path.join(memDir, 'stats.json');
//!   if (!fs.existsSync(f)) return null;
//!   try { return JSON.parse(fs.readFileSync(f, 'utf8')); } catch { return null; }
//! }
//!
//! function saveStats(memDir, stats) {
//!   try {
//!     fs.writeFileSync(path.join(memDir, 'stats.json'),
//!                      JSON.stringify(stats, null, 2));
//!   } catch {}
//! }
//!
//! function incrementStats(memDir, field) {
//!   const stats = loadStats(memDir) || { sessions: 0, observations: 0 };
//!   stats[field] = (stats[field] || 0) + 1;
//!   saveStats(memDir, stats);
//!   return stats;
//! }
//! ```
//!
//! ## Forma dos dados
//!
//! `stats.json` é um objeto JSON livre — callers (ver `session-init.cjs`,
//! `session-end.cjs`, `observe.cjs`, `startup.cjs`) gravam campos
//! heterogêneos:
//!
//! - `sessions`, `observations` — contadores inteiros (manipulados por
//!   [`increment_stats`]).
//! - `last_injection_tokens`, `last_injection_budget` — inteiros.
//! - `last_injection_at` — string ISO datetime.
//! - `last_injection_blocks`, `last_injection_skipped` — arrays de
//!   strings.
//!
//! Por isso expomos a "forma" como [`serde_json::Value`] puro em vez de
//! uma struct fechada — callers futuros podem acrescentar chaves sem
//! precisar mexer neste crate.
//!
//! ## Paridade de serialização
//!
//! `JSON.stringify(obj, null, 2)` do Node e
//! `serde_json::to_string_pretty` emitem **bytes idênticos** para os
//! tipos que aparecem nos stats reais (strings ASCII/UTF-8 sem escape
//! especial, inteiros, arrays rasos). Validado empiricamente antes de
//! portar.
//!
//! ## Paridade de ordem de chaves
//!
//! Com a feature `preserve_order` do `serde_json` (ativa no
//! `core-paths/Cargo.toml`), `Map<String, Value>` passa a ser
//! `IndexMap`, preservando ordem de inserção — idêntico a Node.
//!
//! ## Swallow errors
//!
//! `saveStats` do `.cjs` engole qualquer erro de I/O (`try {} catch {}`).
//! Replicamos: falhas de permissão/disco-cheio somem silenciosamente.
//! Paridade > diagnóstico ideal — se quisermos emitir warning pós-cutover,
//! abrimos feature flag.

use serde_json::{Map, Value};
use std::fs;
use std::path::Path;

/// Nome do arquivo de stats, relativo a `mem_dir`.
pub const STATS_FILE: &str = "stats.json";

/// Lê `<mem_dir>/stats.json` e devolve o `Value` parseado.
///
/// Replica `loadStats(memDir)`:
/// - Arquivo inexistente → `None`.
/// - Leitura falha (permissão, I/O) → `None` (swallow).
/// - JSON malformado → `None` (swallow do `try/catch`).
///
/// **Importante:** o tipo de retorno é `Option<Value>`, não
/// `Option<Map<...>>` — `.cjs` não valida que o conteúdo é objeto. Se
/// alguém gravar `null`, `[1,2,3]` ou `"texto"` no arquivo, `JSON.parse`
/// devolve esse valor e o caller do `.cjs` lida como pode. Mantemos a
/// mesma flexibilidade para paridade em testes pathological.
pub fn load_stats(mem_dir: &Path) -> Option<Value> {
    let f = mem_dir.join(STATS_FILE);
    if !f.exists() {
        return None;
    }
    let content = fs::read_to_string(&f).ok()?;
    serde_json::from_str::<Value>(&content).ok()
}

/// Grava `<mem_dir>/stats.json` com `JSON.stringify(stats, null, 2)`.
///
/// Replica `saveStats(memDir, stats)`:
/// - Serialização falha → silenciado (nunca deveria ocorrer com `Value`
///   bem-formado, mas preserva o `try/catch`).
/// - Escrita falha (permissão, ENOSPC) → silenciado.
///
/// `serde_json::to_string_pretty` usa indent = 2 espaços, `": "` após
/// keys, `",\n"` separando campos — byte-a-byte igual ao Node para os
/// tipos realistas dos callers (int, string, array raso).
pub fn save_stats(mem_dir: &Path, stats: &Value) {
    let path = mem_dir.join(STATS_FILE);
    let Ok(s) = serde_json::to_string_pretty(stats) else {
        return;
    };
    let _ = fs::write(path, s);
}

/// Incrementa `stats[field]` e persiste.
///
/// Replica `incrementStats(memDir, field)`:
/// 1. Carrega stats; se ausente/inválido, usa default
///    `{"sessions": 0, "observations": 0}`.
/// 2. `stats[field] = (stats[field] || 0) + 1`.
/// 3. Salva e retorna o objeto inteiro.
///
/// ### Semântica de incremento
///
/// JS trata falsy-or-zero + 1. Implementamos os casos que acontecem na
/// prática (callers usam `'sessions'` e `'observations'`, ambos contadores
/// inteiros). Para valores não-numéricos existentes (string, array, obj)
/// o `.cjs` faria coerção exótica (`"5" + 1 === "51"`) — registrado como
/// finding F2-11 `migration_findings.md`; aqui tratamos como falsy para
/// não propagar a esquisitice de JS.
///
/// ### Ordem de chaves
///
/// Se `field` já existe no objeto carregado, manuseamos **no lugar**
/// (preserva posição). Se é novo, `insert` anexa ao final — idêntico ao
/// comportamento de `obj[field] = x` em JS quando `field` é novo.
pub fn increment_stats(mem_dir: &Path, field: &str) -> Value {
    let mut stats = match load_stats(mem_dir) {
        Some(v @ Value::Object(_)) => v,
        // Incluindo None, null, array, string, number — cai no default do `|| {...}` do JS
        // (todos eles ou são falsy, ou não têm `[field]` significativo no uso real).
        _ => default_stats(),
    };
    let obj = stats.as_object_mut().expect("garantido Value::Object acima");
    let current = obj.get(field).cloned().unwrap_or(Value::Null);
    let next = increment_js_semantics(&current);
    obj.insert(field.to_string(), next);
    save_stats(mem_dir, &stats);
    stats
}

/// `{"sessions": 0, "observations": 0}` — default quando `stats.json`
/// está ausente ou corrompido. Ordem de chaves é **significativa**
/// (paridade com o literal do `.cjs`).
fn default_stats() -> Value {
    let mut m = Map::new();
    m.insert("sessions".to_string(), Value::Number(0.into()));
    m.insert("observations".to_string(), Value::Number(0.into()));
    Value::Object(m)
}

/// Incremento com semântica JS `(v || 0) + 1` para os casos realistas.
/// Non-numeric truthy cai em `1` — ver F2-11 para discussão.
fn increment_js_semantics(v: &Value) -> Value {
    match v {
        Value::Null => Value::Number(1.into()),
        Value::Bool(false) => Value::Number(1.into()),
        Value::String(s) if s.is_empty() => Value::Number(1.into()),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                // Caminho principal: contador inteiro incrementa.
                Value::Number((i + 1).into())
            } else if let Some(u) = n.as_u64() {
                Value::Number((u + 1).into())
            } else if let Some(f) = n.as_f64() {
                if f == 0.0 || f.is_nan() {
                    // JS: `0 || 0` = 0, `NaN || 0` = 0 — ambos viram 1.
                    Value::Number(1.into())
                } else {
                    serde_json::Number::from_f64(f + 1.0)
                        .map(Value::Number)
                        .unwrap_or_else(|| Value::Number(1.into()))
                }
            } else {
                Value::Number(1.into())
            }
        }
        // Truthy non-numeric (bool true, string não-vazia, array, objeto).
        // Divergência consciente do `.cjs` — ver F2-11.
        _ => Value::Number(1.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn mem(tmp: &TempDir) -> &Path {
        tmp.path()
    }

    #[test]
    fn load_returns_none_when_file_missing() {
        let tmp = TempDir::new().unwrap();
        assert!(load_stats(mem(&tmp)).is_none());
    }

    #[test]
    fn load_returns_none_on_corrupt_json() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(STATS_FILE), "{not json").unwrap();
        assert!(load_stats(mem(&tmp)).is_none());
    }

    #[test]
    fn load_parses_object() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join(STATS_FILE),
            r#"{"sessions": 3, "observations": 7}"#,
        )
        .unwrap();
        let v = load_stats(mem(&tmp)).unwrap();
        assert_eq!(v["sessions"], 3);
        assert_eq!(v["observations"], 7);
    }

    #[test]
    fn save_writes_pretty_json_with_two_space_indent() {
        let tmp = TempDir::new().unwrap();
        let v = serde_json::json!({"sessions": 1, "observations": 2});
        save_stats(mem(&tmp), &v);
        let s = fs::read_to_string(tmp.path().join(STATS_FILE)).unwrap();
        assert_eq!(
            s,
            "{\n  \"sessions\": 1,\n  \"observations\": 2\n}"
        );
    }

    #[test]
    fn save_is_silent_on_unwritable_path() {
        // mem_dir inexistente → write falha, mas função não deve panicar.
        let tmp = TempDir::new().unwrap();
        let bogus = tmp.path().join("nao/existe");
        save_stats(&bogus, &serde_json::json!({}));
        // Se chegou aqui sem panic, passou.
    }

    #[test]
    fn increment_creates_default_when_missing() {
        let tmp = TempDir::new().unwrap();
        let s = increment_stats(mem(&tmp), "sessions");
        assert_eq!(s["sessions"], 1);
        assert_eq!(s["observations"], 0);
        // Ordem preservada no arquivo também.
        let raw = fs::read_to_string(tmp.path().join(STATS_FILE)).unwrap();
        assert!(raw.find("\"sessions\"").unwrap() < raw.find("\"observations\"").unwrap());
    }

    #[test]
    fn increment_adds_new_field_at_end() {
        let tmp = TempDir::new().unwrap();
        let s = increment_stats(mem(&tmp), "custom");
        let raw = fs::read_to_string(tmp.path().join(STATS_FILE)).unwrap();
        let pos_sessions = raw.find("\"sessions\"").unwrap();
        let pos_obs = raw.find("\"observations\"").unwrap();
        let pos_custom = raw.find("\"custom\"").unwrap();
        assert!(pos_sessions < pos_obs);
        assert!(pos_obs < pos_custom);
        assert_eq!(s["custom"], 1);
    }

    #[test]
    fn increment_existing_field_preserves_order() {
        let tmp = TempDir::new().unwrap();
        // Grava stats com "observations" antes de "sessions" (ordem arbitrária).
        fs::write(
            tmp.path().join(STATS_FILE),
            "{\n  \"observations\": 10,\n  \"sessions\": 5\n}",
        )
        .unwrap();
        increment_stats(mem(&tmp), "sessions");
        let raw = fs::read_to_string(tmp.path().join(STATS_FILE)).unwrap();
        let pos_obs = raw.find("\"observations\"").unwrap();
        let pos_sessions = raw.find("\"sessions\"").unwrap();
        // Ordem preservada: observations antes de sessions.
        assert!(pos_obs < pos_sessions);
        assert!(raw.contains("\"sessions\": 6"));
    }

    #[test]
    fn increment_from_zero() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join(STATS_FILE),
            r#"{"sessions": 0, "observations": 0}"#,
        )
        .unwrap();
        let s = increment_stats(mem(&tmp), "sessions");
        assert_eq!(s["sessions"], 1);
    }

    #[test]
    fn increment_twice_accumulates() {
        let tmp = TempDir::new().unwrap();
        increment_stats(mem(&tmp), "sessions");
        let s = increment_stats(mem(&tmp), "sessions");
        assert_eq!(s["sessions"], 2);
    }

    #[test]
    fn increment_null_field_becomes_one() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join(STATS_FILE),
            r#"{"sessions": null, "observations": 0}"#,
        )
        .unwrap();
        let s = increment_stats(mem(&tmp), "sessions");
        // null || 0 = 0, +1 = 1.
        assert_eq!(s["sessions"], 1);
    }

    #[test]
    fn increment_over_corrupt_file_uses_default() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(STATS_FILE), "not json").unwrap();
        let s = increment_stats(mem(&tmp), "observations");
        assert_eq!(s["sessions"], 0);
        assert_eq!(s["observations"], 1);
    }
}
