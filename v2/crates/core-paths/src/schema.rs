//! Versionamento de schema dos arquivos de estado (JSON).
//!
//! Convenção (ver `docs/evolucao-e-integracoes.md`): a **ausência** de
//! `schema_version` equivale à **versão 1** (baseline). Carimbamos `1` nos
//! arquivos novos pra que, quando um shape mudar de verdade, exista o campo
//! pra ramificar a migração.
//!
//! Migração é **lazy**: escrita no `read`/`load`, só quando o shape mudar.
//! Hoje não há migração — apenas o trilho.

use serde_json::Value;

/// Versão atual padrão dos arquivos de estado. Sobe ao introduzir uma
/// migração (e o `load` correspondente passa a fazer upgrade do que vier <).
pub const CURRENT: u32 = 1;

/// Lê `schema_version` de um objeto JSON. Default `1` (baseline) se ausente
/// ou inválido — arquivos pré-versionamento são tratados como v1.
pub fn read_version(v: &Value) -> u32 {
    v.get("schema_version")
        .and_then(Value::as_u64)
        .map(|n| n as u32)
        .unwrap_or(1)
}

/// Carimba `schema_version` num objeto JSON (idempotente). No-op se `v` não
/// for objeto. Use ao montar/salvar um arquivo de estado.
pub fn stamp(v: &mut Value, version: u32) {
    if let Some(obj) = v.as_object_mut() {
        obj.insert("schema_version".into(), Value::from(version));
    }
}
