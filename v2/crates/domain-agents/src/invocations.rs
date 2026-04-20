//! `countInvocations` — porta de `agents.cjs:463-482`.
//!
//! Lê `<mem_dir>/observations.jsonl` e agrega estatísticas dos
//! eventos `agent_call`: contagem por agente e timestamp mais recente
//! por agente.
//!
//! ## Formato de retorno (paridade assimétrica)
//!
//! O `.cjs` retorna dois shapes **diferentes**:
//!
//! - **Arquivo ausente:** `{}` (objeto vazio, sem `counts`/`recent`).
//! - **Arquivo presente:** `{ counts, recent }`.
//!
//! Paridade preservada em [`count_invocations`] retornando `Value`.
//!
//! ## Tratamento de erros (swallow-on-break)
//!
//! O `.cjs` envolve o `for` inteiro em `try { ... } catch {}`. Se
//! qualquer operação lança (ex: `null.type`, `(42).toLowerCase`), o
//! loop **para** e o que já foi acumulado é retornado. Replicamos com
//! `break` explícito nos casos conhecidos.
//!
//! ### Casos que quebram o loop em JS (e portanto em Rust)
//!
//! - Linha é `null` → `null.type` lança `TypeError` → break.
//! - `o.agent` é número não-zero, boolean true, array ou objeto →
//!   `.toLowerCase` não existe → break.
//!
//! ### Casos que apenas pulam a linha (`continue`)
//!
//! - Linha não é JSON válido → `JSON.parse` catch.
//! - `o.type !== 'agent_call'` (incluindo type undefined).
//! - `o.agent` falsy (null, undefined, `0`, `false`, string vazia).
//!
//! ## Comparação de timestamps
//!
//! JS faz `(o.ts || '') > recent[name]` com comparação de string
//! lexicográfica. Timestamps ISO8601 sortam lexicograficamente.
//! Replicamos com `String > String`.

use serde_json::{json, Map, Value};
use std::fs;
use std::path::Path;

/// Conta invocações de agente a partir de `observations.jsonl`.
///
/// Retorna:
/// - `{}` se o arquivo não existe.
/// - `{ "counts": {...}, "recent": {...} }` caso contrário (mesmo se
///   o arquivo for vazio ou sem eventos `agent_call` — `counts` e
///   `recent` serão objetos vazios).
pub fn count_invocations(mem_dir: &Path) -> Value {
    let obs_file = mem_dir.join("observations.jsonl");
    if !obs_file.exists() {
        return json!({});
    }

    let mut counts: Map<String, Value> = Map::new();
    let mut recent: Map<String, Value> = Map::new();

    let body = match fs::read_to_string(&obs_file) {
        Ok(s) => s,
        // Leitura falhou → paridade com outer try/catch: retorna
        // `{counts:{}, recent:{}}` (ambos vazios).
        Err(_) => return json!({ "counts": counts, "recent": recent }),
    };

    for line in body.split('\n').filter(|l| !l.is_empty()) {
        // JSON.parse inner try/catch: linha inválida pula.
        let o: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // JS: `o.type` em `null` lança TypeError → break outer loop.
        if o.is_null() {
            break;
        }

        // JS: `o.type` em primitivos (number, string, bool) é undefined
        // → `!== 'agent_call'` verdadeiro → continue.
        let obj = match o.as_object() {
            Some(m) => m,
            None => continue,
        };

        if obj.get("type").and_then(Value::as_str) != Some("agent_call") {
            continue;
        }

        // `(o.agent || '').toLowerCase()` — coerce falsy to "", call
        // toLowerCase em string; em outros valores lança.
        let name = match obj.get("agent") {
            Some(Value::String(s)) => s.to_lowercase(),
            None | Some(Value::Null) | Some(Value::Bool(false)) => String::new(),
            Some(Value::Number(n)) if n.as_f64() == Some(0.0) => String::new(),
            // number truthy, bool true, array, object → toLowerCase
            // throws em JS → break outer loop.
            Some(_) => break,
        };
        if name.is_empty() {
            continue;
        }

        // counts[name] = (counts[name] || 0) + 1
        let prev = counts.get(&name).and_then(Value::as_u64).unwrap_or(0);
        counts.insert(name.clone(), json!(prev + 1));

        // `(o.ts || '') > recent[name]` — comparação lexicográfica.
        // Se não tem ts, coerce para "".
        let ts_val = obj.get("ts").cloned().unwrap_or(Value::Null);
        let ts_for_compare = ts_val.as_str().unwrap_or("").to_string();

        let update = match recent.get(&name) {
            None => true,
            Some(existing) => {
                // JS `!recent[name]` — undefined/null/''/0/false/NaN são falsy.
                let is_falsy = match existing {
                    Value::Null => true,
                    Value::Bool(false) => true,
                    Value::String(s) if s.is_empty() => true,
                    Value::Number(n) if n.as_f64() == Some(0.0) => true,
                    _ => false,
                };
                if is_falsy {
                    true
                } else {
                    // Comparação string > string. Se existing não for
                    // string, JS coage (primitive → string); para
                    // simplificar e cobrir caso realista, tratamos
                    // non-string como "" (recent[name] começa vindo de
                    // algum o.ts que normalmente é string).
                    let existing_str = existing.as_str().unwrap_or("");
                    ts_for_compare.as_str() > existing_str
                }
            }
        };
        if update {
            recent.insert(name, ts_val);
        }
    }

    json!({
        "counts": Value::Object(counts),
        "recent": Value::Object(recent),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_obs(tmp: &TempDir, body: &str) {
        fs::write(tmp.path().join("observations.jsonl"), body).unwrap();
    }

    #[test]
    fn returns_empty_object_when_missing() {
        let tmp = TempDir::new().unwrap();
        let got = count_invocations(tmp.path());
        assert_eq!(got, json!({}));
    }

    #[test]
    fn empty_file_returns_zero_counts() {
        let tmp = TempDir::new().unwrap();
        write_obs(&tmp, "");
        let got = count_invocations(tmp.path());
        assert_eq!(got, json!({ "counts": {}, "recent": {} }));
    }

    #[test]
    fn counts_single_agent_call() {
        let tmp = TempDir::new().unwrap();
        write_obs(
            &tmp,
            r#"{"type":"agent_call","agent":"manager","ts":"2026-04-18T10:00:00Z"}"#,
        );
        let got = count_invocations(tmp.path());
        assert_eq!(got["counts"]["manager"], json!(1));
        assert_eq!(got["recent"]["manager"], json!("2026-04-18T10:00:00Z"));
    }

    #[test]
    fn counts_accumulate_per_agent() {
        let tmp = TempDir::new().unwrap();
        write_obs(
            &tmp,
            "{\"type\":\"agent_call\",\"agent\":\"manager\",\"ts\":\"2026-04-18T10:00:00Z\"}\n\
             {\"type\":\"agent_call\",\"agent\":\"manager\",\"ts\":\"2026-04-18T11:00:00Z\"}\n\
             {\"type\":\"agent_call\",\"agent\":\"oracle\",\"ts\":\"2026-04-18T12:00:00Z\"}",
        );
        let got = count_invocations(tmp.path());
        assert_eq!(got["counts"]["manager"], json!(2));
        assert_eq!(got["counts"]["oracle"], json!(1));
        assert_eq!(got["recent"]["manager"], json!("2026-04-18T11:00:00Z"));
    }

    #[test]
    fn ignores_non_agent_call_types() {
        let tmp = TempDir::new().unwrap();
        write_obs(
            &tmp,
            "{\"type\":\"other\",\"agent\":\"manager\",\"ts\":\"2026\"}\n\
             {\"type\":\"agent_call\",\"agent\":\"oracle\",\"ts\":\"2026\"}",
        );
        let got = count_invocations(tmp.path());
        assert_eq!(got["counts"].as_object().unwrap().len(), 1);
        assert_eq!(got["counts"]["oracle"], json!(1));
    }

    #[test]
    fn lowercases_agent_name() {
        let tmp = TempDir::new().unwrap();
        write_obs(
            &tmp,
            "{\"type\":\"agent_call\",\"agent\":\"Manager\",\"ts\":\"2026-01\"}\n\
             {\"type\":\"agent_call\",\"agent\":\"MANAGER\",\"ts\":\"2026-02\"}",
        );
        let got = count_invocations(tmp.path());
        assert_eq!(got["counts"]["manager"], json!(2));
    }

    #[test]
    fn skips_empty_agent() {
        let tmp = TempDir::new().unwrap();
        write_obs(
            &tmp,
            "{\"type\":\"agent_call\",\"agent\":\"\",\"ts\":\"2026\"}\n\
             {\"type\":\"agent_call\",\"ts\":\"2026\"}",
        );
        let got = count_invocations(tmp.path());
        assert_eq!(got["counts"].as_object().unwrap().len(), 0);
    }

    #[test]
    fn skips_invalid_json_lines() {
        let tmp = TempDir::new().unwrap();
        write_obs(
            &tmp,
            "not-json\n\
             {\"type\":\"agent_call\",\"agent\":\"manager\",\"ts\":\"2026\"}",
        );
        let got = count_invocations(tmp.path());
        assert_eq!(got["counts"]["manager"], json!(1));
    }

    #[test]
    fn recent_keeps_max_timestamp() {
        let tmp = TempDir::new().unwrap();
        write_obs(
            &tmp,
            "{\"type\":\"agent_call\",\"agent\":\"m\",\"ts\":\"2026-01\"}\n\
             {\"type\":\"agent_call\",\"agent\":\"m\",\"ts\":\"2026-03\"}\n\
             {\"type\":\"agent_call\",\"agent\":\"m\",\"ts\":\"2026-02\"}",
        );
        let got = count_invocations(tmp.path());
        assert_eq!(got["recent"]["m"], json!("2026-03"));
    }

    #[test]
    fn null_line_breaks_loop_preserving_accumulated() {
        // `null.type` lança em JS → break outer loop. Entradas antes
        // permanecem; entradas depois são perdidas.
        let tmp = TempDir::new().unwrap();
        write_obs(
            &tmp,
            "{\"type\":\"agent_call\",\"agent\":\"a\",\"ts\":\"1\"}\n\
             null\n\
             {\"type\":\"agent_call\",\"agent\":\"b\",\"ts\":\"2\"}",
        );
        let got = count_invocations(tmp.path());
        assert_eq!(got["counts"]["a"], json!(1));
        assert!(got["counts"].get("b").is_none());
    }

    #[test]
    fn numeric_agent_breaks_loop() {
        // `(42).toLowerCase` lança em JS → break outer.
        let tmp = TempDir::new().unwrap();
        write_obs(
            &tmp,
            "{\"type\":\"agent_call\",\"agent\":\"a\",\"ts\":\"1\"}\n\
             {\"type\":\"agent_call\",\"agent\":42,\"ts\":\"2\"}\n\
             {\"type\":\"agent_call\",\"agent\":\"b\",\"ts\":\"3\"}",
        );
        let got = count_invocations(tmp.path());
        assert_eq!(got["counts"]["a"], json!(1));
        assert!(got["counts"].get("b").is_none());
    }

    #[test]
    fn skips_line_without_ts() {
        // Sem ts, recent recebe Value::Null e é atualizado pelo próximo.
        let tmp = TempDir::new().unwrap();
        write_obs(
            &tmp,
            "{\"type\":\"agent_call\",\"agent\":\"m\"}\n\
             {\"type\":\"agent_call\",\"agent\":\"m\",\"ts\":\"2026\"}",
        );
        let got = count_invocations(tmp.path());
        assert_eq!(got["counts"]["m"], json!(2));
        assert_eq!(got["recent"]["m"], json!("2026"));
    }

    #[test]
    fn filters_empty_lines() {
        let tmp = TempDir::new().unwrap();
        write_obs(
            &tmp,
            "\n\n{\"type\":\"agent_call\",\"agent\":\"m\",\"ts\":\"1\"}\n\n",
        );
        let got = count_invocations(tmp.path());
        assert_eq!(got["counts"]["m"], json!(1));
    }
}
