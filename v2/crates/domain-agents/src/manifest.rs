//! Manifest I/O — porta de `loadManifest` e `saveManifest` em
//! `agents.cjs:411-420`.
//!
//! ```js
//! function loadManifest(memDir) {
//!   const p = manifestPath(memDir);
//!   if (!fs.existsSync(p)) return null;
//!   try { return JSON.parse(fs.readFileSync(p, 'utf8')); }
//!   catch { return null; }
//! }
//!
//! function saveManifest(memDir, cfg) {
//!   fs.writeFileSync(manifestPath(memDir), JSON.stringify(cfg, null, 2) + '\n');
//! }
//! ```
//!
//! ## Semântica de erros
//!
//! - **load**: arquivo ausente ou JSON inválido → `None`. Nunca propaga
//!   erro, mimicking o `catch {}` do `.cjs`. Permissão negada também
//!   vira `None` (o `.cjs` joga um throw — mas os callers todos
//!   envolvem em try/catch).
//! - **save**: propaga erro de I/O. O `.cjs` joga throw de
//!   `fs.writeFileSync` e o caller decide.
//!
//! ## Formato em disco
//!
//! Pretty-print com indent 2 + trailing `\n` — idêntico ao
//! `JSON.stringify(cfg, null, 2) + '\n'`. Usamos `preserve_order`
//! do serde_json para manter a ordem das chaves do input.

use crate::manifest_path;
use serde_json::Value;
use std::fs;
use std::io;
use std::path::Path;

/// Carrega o manifest de `<mem_dir>/agents.json` ou devolve `None` se
/// ausente/corrompido.
///
/// Replica `loadManifest(memDir)` — mesma semântica de swallow: erro
/// de parse ou de leitura vira `None` para preservar o `try/catch`
/// do `.cjs`.
pub fn load_manifest(mem_dir: &Path) -> Option<Value> {
    let path = manifest_path(mem_dir);
    if !path.exists() {
        return None;
    }
    let raw = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Grava `cfg` em `<mem_dir>/agents.json` como JSON pretty indent 2
/// com trailing `\n`.
///
/// Replica `saveManifest(memDir, cfg)` byte-a-byte. Propaga erro de
/// I/O ou de serialização — o `.cjs` também propaga (não tem
/// try/catch aqui).
pub fn save_manifest(mem_dir: &Path, cfg: &Value) -> io::Result<()> {
    let pretty = serde_json::to_string_pretty(cfg)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let content = format!("{pretty}\n");
    fs::write(manifest_path(mem_dir), content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn load_returns_none_when_file_missing() {
        let tmp = TempDir::new().unwrap();
        assert!(load_manifest(tmp.path()).is_none());
    }

    #[test]
    fn load_returns_none_when_json_invalid() {
        let tmp = TempDir::new().unwrap();
        fs::write(manifest_path(tmp.path()), "{not json").unwrap();
        assert!(load_manifest(tmp.path()).is_none());
    }

    #[test]
    fn load_returns_parsed_value() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            manifest_path(tmp.path()),
            r#"{"version":1,"manager":"manager"}"#,
        )
        .unwrap();
        let got = load_manifest(tmp.path()).unwrap();
        assert_eq!(got["version"], json!(1));
        assert_eq!(got["manager"], json!("manager"));
    }

    #[test]
    fn save_writes_pretty_with_trailing_newline() {
        let tmp = TempDir::new().unwrap();
        let cfg = json!({"version": 1, "agents": {"a": {"x": 1}}});
        save_manifest(tmp.path(), &cfg).unwrap();
        let got = fs::read_to_string(manifest_path(tmp.path())).unwrap();
        assert!(got.ends_with('\n'));
        // 2-space indent.
        assert!(got.contains("\n  \"version\": 1"));
    }

    #[test]
    fn save_then_load_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let cfg = json!({
            "version": 1,
            "manager": "manager",
            "agents": {
                "manager": {"type": "core", "autonomy": {"level": "high"}}
            }
        });
        save_manifest(tmp.path(), &cfg).unwrap();
        let loaded = load_manifest(tmp.path()).unwrap();
        assert_eq!(loaded, cfg);
    }

    #[test]
    fn save_preserves_key_order() {
        // preserve_order garante que z aparece antes de a no disco.
        let tmp = TempDir::new().unwrap();
        let cfg: Value = serde_json::from_str(r#"{"z":1,"a":2,"m":3}"#).unwrap();
        save_manifest(tmp.path(), &cfg).unwrap();
        let got = fs::read_to_string(manifest_path(tmp.path())).unwrap();
        let z_pos = got.find("\"z\"").unwrap();
        let a_pos = got.find("\"a\"").unwrap();
        let m_pos = got.find("\"m\"").unwrap();
        assert!(z_pos < a_pos);
        assert!(a_pos < m_pos);
    }

    #[test]
    fn save_overwrites_existing() {
        let tmp = TempDir::new().unwrap();
        fs::write(manifest_path(tmp.path()), "old content").unwrap();
        save_manifest(tmp.path(), &json!({"x": 1})).unwrap();
        let got = fs::read_to_string(manifest_path(tmp.path())).unwrap();
        assert!(!got.contains("old content"));
        assert!(got.contains("\"x\""));
    }

    #[test]
    fn load_empty_file_returns_none() {
        let tmp = TempDir::new().unwrap();
        fs::write(manifest_path(tmp.path()), "").unwrap();
        assert!(load_manifest(tmp.path()).is_none());
    }

    #[test]
    fn load_whitespace_only_returns_none() {
        let tmp = TempDir::new().unwrap();
        fs::write(manifest_path(tmp.path()), "   \n\t").unwrap();
        assert!(load_manifest(tmp.path()).is_none());
    }
}
