//! Port de `plugin/scripts/integrity.cjs`. Snapshot + diff de arquivos
//! críticos (mtime + size + sha256). Defesa em profundidade contra
//! auto-modificação pelo agente.
//!
//! Diferença do .cjs: o `pluginRoot` é injetado explicitamente ao invés
//! de derivado de `__dirname` — Rust não tem sentido de "dirname do
//! binário". O chamador passa o path absoluto do `plugin/`.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf, MAIN_SEPARATOR};

#[derive(Debug, thiserror::Error)]
pub enum IntegrityError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Relpath (relativos a `plugin_root`) dos arquivos do próprio plugin
/// cuja modificação é violação grave.
pub const PLUGIN_CRITICAL_RELPATHS: &[&str] = &[
    "scripts/agents.cjs",
    "scripts/bus.cjs",
    "scripts/queue.cjs",
    "scripts/router.cjs",
    "scripts/sigilo.cjs",
    "scripts/integrity.cjs",
    "scripts/graph.cjs",
    "scripts/lib.cjs",
    "scripts/nudge.cjs",
    "scripts/best-practices.cjs",
    "scripts/pipelines.cjs",
    "scripts/architect.cjs",
    "scripts/skills-install.cjs",
    "scripts/ide-adapters.cjs",
    "scripts/seed.cjs",
    "ui/server.cjs",
];

/// Relpath (relativos a `mem_dir`) dos manifests críticos do projeto.
pub const PROJECT_CRITICAL_RELPATHS: &[&str] = &[
    "agents.json",
    "bus-config.json",
    "humans.json",
];

/// Resolve a lista absoluta de arquivos do plugin a monitorar, filtrando
/// os inexistentes.
pub fn plugin_critical_files(plugin_root: &Path) -> Vec<PathBuf> {
    PLUGIN_CRITICAL_RELPATHS
        .iter()
        .map(|r| plugin_root.join(r))
        .filter(|p| p.exists())
        .collect()
}

pub fn project_critical_files(mem_dir: &Path) -> Vec<PathBuf> {
    PROJECT_CRITICAL_RELPATHS
        .iter()
        .map(|r| mem_dir.join(r))
        .filter(|p| p.exists())
        .collect()
}

/// Snapshot de um arquivo — mtime/size (rápido) + sha256 (confirmação).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileSnapshot {
    pub mtime: f64,
    pub size: u64,
    pub sha: Option<String>,
}

fn stat_file(p: &Path) -> Option<(f64, u64)> {
    let st = fs::metadata(p).ok()?;
    let mtime = st
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| (d.as_secs() as f64) * 1000.0 + (d.subsec_nanos() as f64) / 1_000_000.0)
        .unwrap_or(0.0);
    Some((mtime, st.len()))
}

fn hash_file(p: &Path) -> Option<String> {
    let bytes = fs::read(p).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Some(hex::encode(hasher.finalize()))
}

/// Snapshot completo. A chave é o path absoluto como string (para
/// paridade com o .cjs que usa `Map<string, ...>`). A ordem de iteração
/// é insertion-order (plugin files primeiro, depois project files).
pub fn snapshot(plugin_root: &Path, mem_dir: &Path) -> BTreeMap<String, FileSnapshot> {
    // Observação: .cjs usa Map nativo que preserva insertion-order. Para
    // paridade do `diff`, o consumidor não enxerga ordem (é um set de
    // mudanças). Usamos BTreeMap pra determinismo extra.
    let mut out: BTreeMap<String, FileSnapshot> = BTreeMap::new();
    let targets: Vec<PathBuf> = plugin_critical_files(plugin_root)
        .into_iter()
        .chain(project_critical_files(mem_dir))
        .collect();
    for p in targets {
        let Some((mtime, size)) = stat_file(&p) else { continue };
        let sha = hash_file(&p);
        out.insert(
            p.to_string_lossy().into_owned(),
            FileSnapshot { mtime, size, sha },
        );
    }
    out
}

/// Tipo de mudança detectada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    Modified,
    Deleted,
    Created,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Change {
    pub path: String,
    pub change: ChangeKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_delta: Option<i64>,
}

/// Compara dois snapshots. Espelha byte-a-byte o comportamento do .cjs:
/// - `deleted` se existia antes e não depois.
/// - `created` se existe depois mas não antes.
/// - `modified` se mtime+size divergem E sha256 diverge (confirmação).
pub fn diff(
    before: &BTreeMap<String, FileSnapshot>,
    after: &BTreeMap<String, FileSnapshot>,
) -> Vec<Change> {
    let mut changes = Vec::new();
    let mut all_keys: std::collections::BTreeSet<&String> = std::collections::BTreeSet::new();
    for k in before.keys() {
        all_keys.insert(k);
    }
    for k in after.keys() {
        all_keys.insert(k);
    }
    for p in all_keys {
        let b = before.get(p);
        let a = after.get(p);
        match (b, a) {
            (Some(_), None) => {
                changes.push(Change {
                    path: p.clone(),
                    change: ChangeKind::Deleted,
                    size_delta: None,
                });
            }
            (None, Some(_)) => {
                changes.push(Change {
                    path: p.clone(),
                    change: ChangeKind::Created,
                    size_delta: None,
                });
            }
            (Some(b), Some(a)) => {
                if ((b.mtime - a.mtime).abs() > f64::EPSILON || b.size != a.size)
                    && b.sha != a.sha
                {
                    changes.push(Change {
                        path: p.clone(),
                        change: ChangeKind::Modified,
                        size_delta: Some(a.size as i64 - b.size as i64),
                    });
                }
            }
            (None, None) => {}
        }
    }
    changes
}

/// Categoriza um path em `plugin` / `agents_manifest` / `bus_config` /
/// `humans_manifest` / `other`. Mesmo algoritmo do .cjs.
pub fn classify(abs_path: &str, plugin_root: &Path) -> &'static str {
    let root = plugin_root.to_string_lossy();
    let prefix = format!("{}{}", root, MAIN_SEPARATOR);
    if abs_path.starts_with(&prefix) {
        return "plugin";
    }
    if abs_path.ends_with("/agents.json") {
        return "agents_manifest";
    }
    if abs_path.ends_with("/bus-config.json") {
        return "bus_config";
    }
    if abs_path.ends_with("/humans.json") {
        return "humans_manifest";
    }
    "other"
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn seed(p: &Path, content: &[u8]) {
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(p, content).unwrap();
    }

    #[test]
    fn snapshot_empty_dirs() {
        let plugin = TempDir::new().unwrap();
        let mem = TempDir::new().unwrap();
        let s = snapshot(plugin.path(), mem.path());
        assert!(s.is_empty());
    }

    #[test]
    fn snapshot_includes_existing_files() {
        let plugin = TempDir::new().unwrap();
        let mem = TempDir::new().unwrap();
        seed(&plugin.path().join("scripts/agents.cjs"), b"agents");
        seed(&plugin.path().join("ui/server.cjs"), b"srv");
        seed(&mem.path().join("agents.json"), b"{}");
        let s = snapshot(plugin.path(), mem.path());
        assert_eq!(s.len(), 3);
    }

    #[test]
    fn diff_detects_creation() {
        let plugin = TempDir::new().unwrap();
        let mem = TempDir::new().unwrap();
        let before = snapshot(plugin.path(), mem.path());
        seed(&plugin.path().join("scripts/agents.cjs"), b"agents");
        let after = snapshot(plugin.path(), mem.path());
        let d = diff(&before, &after);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].change, ChangeKind::Created);
    }

    #[test]
    fn diff_detects_deletion() {
        let plugin = TempDir::new().unwrap();
        let mem = TempDir::new().unwrap();
        let f = plugin.path().join("scripts/agents.cjs");
        seed(&f, b"agents");
        let before = snapshot(plugin.path(), mem.path());
        fs::remove_file(&f).unwrap();
        let after = snapshot(plugin.path(), mem.path());
        let d = diff(&before, &after);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].change, ChangeKind::Deleted);
    }

    #[test]
    fn diff_detects_modification() {
        let plugin = TempDir::new().unwrap();
        let mem = TempDir::new().unwrap();
        let f = plugin.path().join("scripts/agents.cjs");
        seed(&f, b"original");
        let before = snapshot(plugin.path(), mem.path());
        // Precisamos garantir que mtime mude; dorme 10ms
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(&f, b"changed").unwrap();
        let after = snapshot(plugin.path(), mem.path());
        let d = diff(&before, &after);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].change, ChangeKind::Modified);
        assert_eq!(d[0].size_delta, Some(b"changed".len() as i64 - b"original".len() as i64));
    }

    #[test]
    fn diff_ignores_same_content() {
        let plugin = TempDir::new().unwrap();
        let mem = TempDir::new().unwrap();
        seed(&plugin.path().join("scripts/agents.cjs"), b"same");
        let before = snapshot(plugin.path(), mem.path());
        let after = snapshot(plugin.path(), mem.path());
        assert!(diff(&before, &after).is_empty());
    }

    #[test]
    fn classify_plugin_file() {
        let plugin = PathBuf::from("/tmp/foo/plugin");
        let abs = format!("/tmp/foo/plugin{}scripts/agents.cjs", MAIN_SEPARATOR);
        assert_eq!(classify(&abs, &plugin), "plugin");
    }

    #[test]
    fn classify_manifests() {
        let plugin = PathBuf::from("/tmp/foo/plugin");
        assert_eq!(classify("/x/agents.json", &plugin), "agents_manifest");
        assert_eq!(classify("/x/bus-config.json", &plugin), "bus_config");
        assert_eq!(classify("/x/humans.json", &plugin), "humans_manifest");
        assert_eq!(classify("/x/other.txt", &plugin), "other");
    }

    #[test]
    fn critical_files_filters_missing() {
        let plugin = TempDir::new().unwrap();
        // Nenhum arquivo ainda
        assert_eq!(plugin_critical_files(plugin.path()).len(), 0);
        seed(&plugin.path().join("scripts/lib.cjs"), b"x");
        assert_eq!(plugin_critical_files(plugin.path()).len(), 1);
    }
}
