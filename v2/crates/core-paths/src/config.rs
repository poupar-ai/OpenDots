//! Project config — porta de `loadProjectConfig` / `saveProjectConfig` /
//! `initProjectConfig` em `lib.cjs`.
//!
//! ```js
//! function loadProjectConfig(memDir) {
//!   const f = path.join(memDir, 'config.json');
//!   if (!fs.existsSync(f)) return null;
//!   try { return JSON.parse(fs.readFileSync(f, 'utf8')); } catch { return null; }
//! }
//!
//! function saveProjectConfig(memDir, cfg) {
//!   fs.writeFileSync(path.join(memDir, 'config.json'),
//!                    JSON.stringify(cfg, null, 2) + '\n');
//! }
//!
//! function initProjectConfig(memDir, projectName) {
//!   const existing = loadProjectConfig(memDir);
//!   if (existing) return existing;
//!   const cfg = {
//!     version: '1',
//!     project: projectName,
//!     created_at: new Date().toISOString()
//!   };
//!   saveProjectConfig(memDir, cfg);
//!   return cfg;
//! }
//! ```
//!
//! ## Diferenças vs `stats.rs`
//!
//! - `config.json` é gravado com **trailing newline** (`'\n'` concatenado
//!   ao fim), enquanto `stats.json` não tem. Byte sutil mas importante
//!   para paridade byte-a-byte.
//! - `saveProjectConfig` **não engole erro** (`try/catch` ausente) — ao
//!   contrário de `saveStats`. Expomos `Result` para propagar falha.
//! - `initProjectConfig` é **idempotente**: se já existir, devolve o
//!   existente sem tocar no disco. Não sobrescreve.
//!
//! ## Injeção de relógio
//!
//! [`init_project_config`] aceita `now_iso: &str` em vez de chamar um
//! clock interno, para teste determinístico e inversão de dependências
//! (o caller decide qual clock usar — `core-ports::Clock::now_iso` na
//! produção). Paridade com `new Date().toISOString()` é validada em
//! parity test via stub de `Date.prototype.toISOString` no script node.

use serde_json::{Map, Value};
use std::fs;
use std::path::Path;

/// Nome do arquivo de config, relativo a `mem_dir`.
pub const CONFIG_FILE: &str = "config.json";

/// Replica `if (value)` em JavaScript.
///
/// Falsy em JS: `null`, `false`, `0`, `""`, `NaN`, `undefined`.
/// Truthy em JS: todo o resto — incluindo `[]`, `{}`, `"0"`, `" "`.
fn is_js_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                f != 0.0 && !f.is_nan()
            } else {
                // i64/u64 que não cabe em f64 — na prática sempre truthy
                // (número grande ≠ 0). Mantemos truthy por consistência.
                true
            }
        }
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// Erros de I/O ao gravar config. Fino wrapper em `std::io::Error` +
/// serialização (praticamente nunca falha com `Value` bem-formado).
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
}

/// Lê `<mem_dir>/config.json` e devolve o `Value` parseado.
///
/// Replica `loadProjectConfig(memDir)`:
/// - Arquivo inexistente → `None`.
/// - Leitura falha (permissão, I/O) → `None` (swallow).
/// - JSON malformado → `None` (swallow).
///
/// Retorna `Option<Value>` — `.cjs` aceita qualquer JSON; tipicamente
/// objeto com `version`/`project`/`created_at`, mas não valida forma.
pub fn load_project_config(mem_dir: &Path) -> Option<Value> {
    let f = mem_dir.join(CONFIG_FILE);
    if !f.exists() {
        return None;
    }
    let content = fs::read_to_string(&f).ok()?;
    serde_json::from_str::<Value>(&content).ok()
}

/// Grava `<mem_dir>/config.json` com `JSON.stringify(cfg, null, 2) + '\n'`.
///
/// Replica `saveProjectConfig(memDir, cfg)`. **Propaga erro** de I/O —
/// diferente de `save_stats` que engole. Paridade com `.cjs` que também
/// propaga (não tem `try/catch`).
///
/// ### Trailing newline
///
/// `.cjs` adiciona `+ '\n'` ao resultado de `JSON.stringify`. Preservar
/// byte-a-byte é crítico: editores POSIX esperam newline final.
pub fn save_project_config(mem_dir: &Path, cfg: &Value) -> Result<(), ConfigError> {
    let path = mem_dir.join(CONFIG_FILE);
    let mut s = serde_json::to_string_pretty(cfg)?;
    s.push('\n');
    fs::write(path, s)?;
    Ok(())
}

/// Inicializa `config.json` se não existe; idempotente.
///
/// Replica `initProjectConfig(memDir, projectName)`:
/// 1. Carrega config existente; se presente, retorna como está (no-op).
/// 2. Senão, monta `{version:"1", project:<name>, created_at:<now_iso>}`
///    e persiste.
///
/// ### `now_iso` como parâmetro
///
/// O `.cjs` chama `new Date().toISOString()` internamente. Aqui injetamos
/// o ISO já formatado para:
/// - **Testes determinísticos** sem mock de relógio.
/// - **Inversão de dependência** — caller da produção usa
///   `core_ports::Clock::now_iso()`.
///
/// Formato esperado: ISO 8601 com millisegundos e `Z`, ex.
/// `"2026-04-19T03:00:00.000Z"` — idêntico ao output de
/// `Date.prototype.toISOString`.
///
/// ### Retorno
///
/// Devolve o config ativo (carregado ou recém-criado). Ordem das chaves
/// no recém-criado é significativa: `version`, `project`, `created_at`.
pub fn init_project_config(
    mem_dir: &Path,
    project_name: &str,
    now_iso: &str,
) -> Result<Value, ConfigError> {
    // `.cjs` faz `if (existing)` — semântica truthy JS, não só presença.
    // `null`, `false`, `0`, `""`, `NaN` no arquivo → falsy → overwrite.
    // Objetos e arrays vazios → truthy → preservam. Replicamos
    // literalmente via `is_js_truthy`.
    if let Some(existing) = load_project_config(mem_dir) {
        if is_js_truthy(&existing) {
            return Ok(existing);
        }
    }
    let mut cfg = Map::new();
    cfg.insert("version".to_string(), Value::String("1".to_string()));
    cfg.insert("project".to_string(), Value::String(project_name.to_string()));
    cfg.insert("created_at".to_string(), Value::String(now_iso.to_string()));
    let value = Value::Object(cfg);
    save_project_config(mem_dir, &value)?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const FIXED_ISO: &str = "2026-04-19T03:00:00.000Z";

    #[test]
    fn load_returns_none_when_file_missing() {
        let tmp = TempDir::new().unwrap();
        assert!(load_project_config(tmp.path()).is_none());
    }

    #[test]
    fn load_returns_none_on_corrupt_json() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(CONFIG_FILE), "{bad json").unwrap();
        assert!(load_project_config(tmp.path()).is_none());
    }

    #[test]
    fn load_parses_object() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join(CONFIG_FILE),
            r#"{"version": "1", "project": "foo"}"#,
        )
        .unwrap();
        let v = load_project_config(tmp.path()).unwrap();
        assert_eq!(v["version"], "1");
        assert_eq!(v["project"], "foo");
    }

    #[test]
    fn save_writes_pretty_with_trailing_newline() {
        let tmp = TempDir::new().unwrap();
        let v = serde_json::json!({"version": "1", "project": "x"});
        save_project_config(tmp.path(), &v).unwrap();
        let s = fs::read_to_string(tmp.path().join(CONFIG_FILE)).unwrap();
        assert_eq!(s, "{\n  \"version\": \"1\",\n  \"project\": \"x\"\n}\n");
        assert!(s.ends_with('\n'), "deve terminar com \\n");
    }

    #[test]
    fn save_propagates_io_error() {
        // mem_dir inexistente (pai não existe) → write falha. Diferente
        // de save_stats que engole.
        let tmp = TempDir::new().unwrap();
        let bogus = tmp.path().join("nao/existe");
        let res = save_project_config(&bogus, &serde_json::json!({}));
        assert!(res.is_err(), "save deve propagar erro");
    }

    #[test]
    fn init_creates_when_missing() {
        let tmp = TempDir::new().unwrap();
        let cfg = init_project_config(tmp.path(), "bankin", FIXED_ISO).unwrap();
        assert_eq!(cfg["version"], "1");
        assert_eq!(cfg["project"], "bankin");
        assert_eq!(cfg["created_at"], FIXED_ISO);
        // Arquivo persistido com conteúdo correto.
        let raw = fs::read_to_string(tmp.path().join(CONFIG_FILE)).unwrap();
        assert_eq!(
            raw,
            "{\n  \"version\": \"1\",\n  \"project\": \"bankin\",\n  \"created_at\": \"2026-04-19T03:00:00.000Z\"\n}\n"
        );
    }

    #[test]
    fn init_is_idempotent_returns_existing() {
        let tmp = TempDir::new().unwrap();
        // Primeira init grava.
        let first = init_project_config(tmp.path(), "foo", FIXED_ISO).unwrap();
        // Sleep não necessário — mudamos o projectName e now_iso; se não
        // for idempotente, sobrescreveria.
        let second =
            init_project_config(tmp.path(), "bar", "2030-01-01T00:00:00.000Z").unwrap();
        assert_eq!(first, second);
        assert_eq!(second["project"], "foo", "idempotente: manteve foo");
        assert_eq!(second["created_at"], FIXED_ISO);
    }

    #[test]
    fn init_over_corrupt_returns_fallback_and_overwrites() {
        // Se config.json existe mas parse falha, loadProjectConfig → None,
        // initProjectConfig trata como "não existe" e sobrescreve.
        // Mesmo bug conceitual que F2-12 (stats) — replicado para paridade.
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(CONFIG_FILE), "corrupt").unwrap();
        let cfg = init_project_config(tmp.path(), "novo", FIXED_ISO).unwrap();
        assert_eq!(cfg["project"], "novo");
        // Arquivo antigo foi sobrescrito.
        let raw = fs::read_to_string(tmp.path().join(CONFIG_FILE)).unwrap();
        assert!(raw.contains("\"project\": \"novo\""));
    }

    #[test]
    fn init_overwrites_when_existing_is_json_null() {
        // JSON `null` no arquivo → JS trata como falsy → overwrite.
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(CONFIG_FILE), "null").unwrap();
        let cfg = init_project_config(tmp.path(), "novo", FIXED_ISO).unwrap();
        assert_eq!(cfg["project"], "novo");
    }

    #[test]
    fn init_overwrites_when_existing_is_zero() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(CONFIG_FILE), "0").unwrap();
        let cfg = init_project_config(tmp.path(), "novo", FIXED_ISO).unwrap();
        assert_eq!(cfg["project"], "novo");
    }

    #[test]
    fn init_overwrites_when_existing_is_false() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(CONFIG_FILE), "false").unwrap();
        let cfg = init_project_config(tmp.path(), "novo", FIXED_ISO).unwrap();
        assert_eq!(cfg["project"], "novo");
    }

    #[test]
    fn init_overwrites_when_existing_is_empty_string() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(CONFIG_FILE), "\"\"").unwrap();
        let cfg = init_project_config(tmp.path(), "novo", FIXED_ISO).unwrap();
        assert_eq!(cfg["project"], "novo");
    }

    #[test]
    fn init_preserves_when_existing_is_truthy_string() {
        // String não-vazia é truthy em JS → .cjs retorna a string.
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(CONFIG_FILE), "\"hello\"").unwrap();
        let cfg = init_project_config(tmp.path(), "novo", FIXED_ISO).unwrap();
        assert_eq!(cfg, serde_json::Value::String("hello".to_string()));
    }

    #[test]
    fn init_preserves_when_existing_is_empty_object() {
        // `{}` é truthy em JS (diferente de Python!) → preserva.
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(CONFIG_FILE), "{}").unwrap();
        let cfg = init_project_config(tmp.path(), "novo", FIXED_ISO).unwrap();
        assert!(cfg.as_object().unwrap().is_empty());
    }

    #[test]
    fn init_preserves_when_existing_is_empty_array() {
        // `[]` é truthy em JS → preserva (sim, `![]` é false em JS).
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(CONFIG_FILE), "[]").unwrap();
        let cfg = init_project_config(tmp.path(), "novo", FIXED_ISO).unwrap();
        assert!(cfg.as_array().unwrap().is_empty());
    }

    #[test]
    fn init_preserves_key_order_version_project_created() {
        let tmp = TempDir::new().unwrap();
        init_project_config(tmp.path(), "proj", FIXED_ISO).unwrap();
        let raw = fs::read_to_string(tmp.path().join(CONFIG_FILE)).unwrap();
        let pos_v = raw.find("\"version\"").unwrap();
        let pos_p = raw.find("\"project\"").unwrap();
        let pos_c = raw.find("\"created_at\"").unwrap();
        assert!(pos_v < pos_p);
        assert!(pos_p < pos_c);
    }
}
