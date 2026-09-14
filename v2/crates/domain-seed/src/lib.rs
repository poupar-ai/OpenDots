//! Seed packs — agentes-template instaláveis por projeto.
//!
//! Porta de `plugin/scripts/seed.cjs` (parte de domínio; o wizard CLI e o
//! parser de argv ficam no adapter).
//!
//! Mudanças de API:
//! - `SEEDS_DIR` era `__dirname/../seeds`. Em Rust vira `seeds_dir: &Path`.
//! - `best-practices` raiz é passada como `bp_dir: &Path` (antes vinha de
//!   `best-practices.cjs` internamente via `__dirname`).
//! - `skills` agora é `Option<SkillsFilter>` — `None` = skip,
//!   `Some(All)` = todas, `Some(Only(vec))` = filtro explícito.
//! - `ide` é `Option<&str>` — chamada a `ide_adapters::apply` só quando
//!   `Some`. Erros do IDE são propagados em `Error::Ide`.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

use domain_best_practices::{apply_practices, ApplyPracticesError, ApplyPracticesResult};
use domain_ide_adapters::{apply as ide_apply, ApplyResult as IdeApplyResult, IdeError};
use domain_skills_install::{apply_skills, ApplyInput as SkillsInput, ApplyResult as SkillsResult};

#[derive(Debug, Error)]
pub enum Error {
    #[error("referência inválida: \"{0}\". Use <categoria>/<pack>.")]
    InvalidRef(String),
    #[error("pack \"{0}\" não existe em {1}")]
    PackMissing(String, PathBuf),
    #[error("pack \"{0}\" sem agents.json")]
    ManifestMissing(String),
    #[error("agents.json inválido: {0}")]
    ManifestInvalid(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("best-practices: {0}")]
    Practices(#[from] ApplyPracticesError),
    #[error("ide: {0}")]
    Ide(#[from] IdeError),
}

// ── Meta helpers ─────────────────────────────────────────────────────────────

/// Lê `dir/meta.json` como objeto JSON. Retorna `{}` se ausente ou malformado.
pub fn read_meta(dir: &Path) -> Map<String, Value> {
    let p = dir.join("meta.json");
    if !p.exists() {
        return Map::new();
    }
    match fs::read_to_string(&p).ok().and_then(|s| {
        let parsed: Option<Value> = serde_json::from_str(&s).ok();
        parsed
    }) {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

// ── Discovery ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub name: String,
    /// Meta ficam achatados no objeto do lado do `.cjs` via spread.
    /// Em Rust expomos separados pra tipar; parity test valida o merge.
    #[serde(flatten)]
    pub meta: Map<String, Value>,
}

/// `listCategories` do `.cjs`: diretórios em `SEEDS_DIR/`, cada um com seu
/// `meta.json` merged na raiz do objeto.
pub fn list_categories(seeds_dir: &Path) -> Vec<Category> {
    if !seeds_dir.exists() {
        return Vec::new();
    }
    let mut entries: Vec<_> = match fs::read_dir(seeds_dir) {
        Ok(it) => it.flatten().collect(),
        Err(_) => return Vec::new(),
    };
    entries.sort_by_key(|e| e.file_name());
    entries
        .into_iter()
        .filter_map(|entry| {
            let p = entry.path();
            if !p.is_dir() {
                return None;
            }
            let meta = read_meta(&p);
            Some(Category {
                name: entry.file_name().to_string_lossy().into_owned(),
                meta,
            })
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pack {
    pub name: String,
    pub dir: PathBuf,
    pub meta: Map<String, Value>,
    pub manifest: Value,
    pub agent_names: Vec<String>,
}

/// `listPacks(category)` do `.cjs`.
pub fn list_packs(seeds_dir: &Path, category: &str) -> Vec<Pack> {
    let cat_dir = seeds_dir.join(category);
    if !cat_dir.exists() {
        return Vec::new();
    }
    let mut entries: Vec<_> = match fs::read_dir(&cat_dir) {
        Ok(it) => it.flatten().collect(),
        Err(_) => return Vec::new(),
    };
    entries.sort_by_key(|e| e.file_name());
    entries
        .into_iter()
        .filter_map(|entry| {
            let p = entry.path();
            if !p.is_dir() {
                return None;
            }
            let agents_file = p.join("agents.json");
            if !agents_file.exists() {
                return None;
            }
            let manifest_raw = fs::read_to_string(&agents_file).ok()?;
            let manifest: Value = serde_json::from_str(&manifest_raw).ok()?;
            let meta = read_meta(&p);
            let agent_names = manifest
                .get("agents")
                .and_then(Value::as_object)
                .map(|m| m.keys().cloned().collect())
                .unwrap_or_default();
            Some(Pack {
                name: entry.file_name().to_string_lossy().into_owned(),
                dir: p,
                meta,
                manifest,
                agent_names,
            })
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct PackRef {
    pub category: String,
    pub pack: String,
    pub dir: PathBuf,
    pub manifest: Value,
    pub meta: Map<String, Value>,
}

/// `loadPackRef` do `.cjs`.
pub fn load_pack_ref(seeds_dir: &Path, reference: &str) -> Result<PackRef, Error> {
    let mut parts = reference.splitn(2, '/');
    let category = parts.next().unwrap_or("");
    let pack = parts.next().unwrap_or("");
    if category.is_empty() || pack.is_empty() {
        return Err(Error::InvalidRef(reference.to_string()));
    }
    let dir = seeds_dir.join(category).join(pack);
    if !dir.exists() {
        return Err(Error::PackMissing(
            reference.to_string(),
            seeds_dir.to_path_buf(),
        ));
    }
    let manifest_path = dir.join("agents.json");
    if !manifest_path.exists() {
        return Err(Error::ManifestMissing(reference.to_string()));
    }
    let raw = fs::read_to_string(&manifest_path)?;
    let manifest: Value =
        serde_json::from_str(&raw).map_err(|e| Error::ManifestInvalid(e.to_string()))?;
    let meta = read_meta(&dir);
    Ok(PackRef {
        category: category.to_string(),
        pack: pack.to_string(),
        dir,
        manifest,
        meta,
    })
}

// ── Filter ───────────────────────────────────────────────────────────────────

fn split_csv(s: Option<&str>) -> Vec<String> {
    s.unwrap_or("")
        .split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

/// Emula `filterAgents(agents, only, exclude)` do `.cjs`.
/// - `only`/`exclude`: strings CSV no formato `"a,b,c"`.
/// - Se `only` não vazio: só keys listadas.
/// - Se `exclude` não vazio: exclui as listadas.
pub fn filter_agents(
    agents: &Map<String, Value>,
    only: Option<&str>,
    exclude: Option<&str>,
) -> Map<String, Value> {
    let only = split_csv(only);
    let exclude = split_csv(exclude);
    let mut out = Map::new();
    for (name, cfg) in agents {
        if !only.is_empty() && !only.iter().any(|n| n == name) {
            continue;
        }
        if !exclude.is_empty() && exclude.iter().any(|n| n == name) {
            continue;
        }
        out.insert(name.clone(), cfg.clone());
    }
    out
}

// ── Apply ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillsFilter {
    All,
    Only(Vec<String>),
}

pub struct ApplyInput<'a> {
    pub seeds_dir: &'a Path,
    pub bp_dir: &'a Path,
    pub reference: &'a str,
    pub project_root: &'a Path,
    pub mem_dir_name: &'a str,
    pub only: Option<&'a str>,
    pub exclude: Option<&'a str>,
    pub overwrite: bool,
    /// Quando true E há colisão de nome, instala como agente novo com
    /// suffix `-2`, `-3`, … (procura o próximo disponível). Mutuamente
    /// exclusivo com `overwrite` na prática — UI garante.
    pub add_new: bool,
    pub skills: Option<SkillsFilter>,
    pub ide: Option<&'a str>,
}

/// Computa o próximo nome disponível com suffix incremental.
/// Se `base` não colide, retorna `base`. Senão tenta `base-2`, `base-3`, ...
fn next_available_name(base: &str, existing: &Map<String, Value>) -> String {
    if !existing.contains_key(base) {
        return base.to_string();
    }
    let mut n: u32 = 2;
    loop {
        let candidate = format!("{base}-{n}");
        if !existing.contains_key(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

#[derive(Debug, Clone)]
pub struct ApplyResult {
    pub added: Vec<String>,
    pub replaced: Vec<String>,
    pub skipped: Vec<String>,
    pub md_copied: Vec<String>,
    pub md_skipped: Vec<String>,
    pub mem_dir: PathBuf,
    pub practices: ApplyPracticesResult,
    pub skills: SkillsResult,
    pub ide: Option<IdeApplyResult>,
}

pub fn apply(input: ApplyInput) -> Result<ApplyResult, Error> {
    let ApplyInput {
        seeds_dir,
        bp_dir,
        reference,
        project_root,
        mem_dir_name,
        only,
        exclude,
        overwrite,
        add_new,
        skills,
        ide,
    } = input;

    let pack_ref = load_pack_ref(seeds_dir, reference)?;
    let src = &pack_ref.manifest;

    let mem_dir = project_root.join(mem_dir_name);
    if !mem_dir.exists() {
        fs::create_dir_all(&mem_dir)?;
    }

    let agents_dir = mem_dir.join("agents");
    if !agents_dir.exists() {
        fs::create_dir_all(&agents_dir)?;
    }

    let target_manifest_path = mem_dir.join("agents.json");
    let mut target: Value = if target_manifest_path.exists() {
        let raw = fs::read_to_string(&target_manifest_path)?;
        serde_json::from_str(&raw)?
    } else {
        let mut m = Map::new();
        m.insert("version".into(), Value::Number(1.into()));
        let manager = src
            .get("manager")
            .cloned()
            .unwrap_or(Value::String("manager".into()));
        let oracle = src
            .get("oracle")
            .cloned()
            .unwrap_or(Value::String("oracle".into()));
        m.insert("manager".into(), manager);
        m.insert("oracle".into(), oracle);
        if let Some(rule) = src.get("rule") {
            m.insert("rule".into(), rule.clone());
        }
        m.insert("agents".into(), Value::Object(Map::new()));
        Value::Object(m)
    };
    {
        let obj = target
            .as_object_mut()
            .ok_or_else(|| Error::ManifestInvalid("target agents.json não é objeto".into()))?;
        if !obj.contains_key("agents") {
            obj.insert("agents".into(), Value::Object(Map::new()));
        }
    }

    let src_agents_all = src
        .get("agents")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let src_agents = filter_agents(&src_agents_all, only, exclude);

    let mut added = Vec::new();
    let mut skipped = Vec::new();
    let mut replaced = Vec::new();
    let mut md_copied = Vec::new();
    let mut md_skipped = Vec::new();
    // Mapeia nome de origem → nome final (após suffix, se aplicável). Usado
    // pela seção `_meta.agent_origin` mais abaixo pra rastrear cada agente
    // instalado de fato (incluindo os renomeados em modo add_new).
    let mut final_names: Vec<(String, String)> = Vec::with_capacity(src_agents.len());

    for (name, cfg) in &src_agents {
        let target_agents = target
            .get_mut("agents")
            .and_then(Value::as_object_mut)
            .expect("garantido acima");
        let exists = target_agents.contains_key(name);

        // Resolve nome final + ação. Ordem de precedência: add_new > overwrite.
        let (final_name, action): (String, &'static str) = if !exists {
            (name.clone(), "added")
        } else if add_new {
            (next_available_name(name, target_agents), "added")
        } else if overwrite {
            (name.clone(), "replaced")
        } else {
            (name.clone(), "skipped")
        };

        let renamed = final_name != *name;

        if action == "skipped" {
            skipped.push(final_name.clone());
        } else {
            // Quando renomeado (final_name != name), ajusta `context_file` no
            // cfg pra apontar pro .md com nome novo. Sem isso o agente
            // carregaria o context_file do irmão original.
            let mut cfg_to_write = cfg.clone();
            if renamed {
                if let Some(obj) = cfg_to_write.as_object_mut() {
                    obj.insert(
                        "context_file".into(),
                        Value::String(format!("agents/{final_name}.md")),
                    );
                }
            }
            target_agents.insert(final_name.clone(), cfg_to_write);

            if action == "replaced" {
                replaced.push(final_name.clone());
            } else {
                added.push(final_name.clone());
            }
        }
        final_names.push((name.clone(), final_name.clone()));

        // Copia o .md de origem pro destino. Em modo add_new, o destino tem
        // o nome novo (`agents/manager-2.md`), então é "sempre novo arquivo".
        // Quando skipped/replaced sem rename, respeita o flag overwrite —
        // mantendo a paridade com o `.cjs` (skipped → md_skipped, replaced
        // c/ overwrite → md_copied).
        let src_rel = cfg
            .get("context_file")
            .and_then(Value::as_str)
            .map(String::from)
            .unwrap_or_else(|| format!("agents/{name}.md"));
        let dst_rel = if renamed {
            format!("agents/{final_name}.md")
        } else {
            src_rel.clone()
        };
        let src_md = pack_ref.dir.join(&src_rel);
        let dst_md = mem_dir.join(&dst_rel);
        if src_md.exists() {
            if let Some(parent) = dst_md.parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent)?;
                }
            }
            if !dst_md.exists() || overwrite || renamed {
                fs::copy(&src_md, &dst_md)?;
                md_copied.push(dst_rel);
            } else {
                md_skipped.push(dst_rel);
            }
        }
    }

    {
        let obj = target.as_object_mut().unwrap();
        if obj.get("manager").map(Value::is_null).unwrap_or(true) {
            if let Some(m) = src.get("manager") {
                obj.insert("manager".into(), m.clone());
            }
        }
        if obj.get("oracle").map(Value::is_null).unwrap_or(true) {
            if let Some(o) = src.get("oracle") {
                obj.insert("oracle".into(), o.clone());
            }
        }
        if obj.get("rule").map(Value::is_null).unwrap_or(true) {
            if let Some(r) = src.get("rule") {
                obj.insert("rule".into(), r.clone());
            }
        }
    }

    // _meta.agent_origin trace — só se ref é "cat/slug" válido.
    let mut parts = reference.splitn(2, '/');
    let seed_category = parts.next().unwrap_or("");
    let seed_slug = parts.next().unwrap_or("");
    if !seed_category.is_empty() && !seed_slug.is_empty() {
        let obj = target.as_object_mut().unwrap();
        if !obj.contains_key("_meta") {
            obj.insert("_meta".into(), Value::Object(Map::new()));
        }
        let meta = obj.get_mut("_meta").and_then(Value::as_object_mut).unwrap();
        if !meta.contains_key("agent_origin") {
            meta.insert("agent_origin".into(), Value::Object(Map::new()));
        }
        let origin = meta
            .get_mut("agent_origin")
            .and_then(Value::as_object_mut)
            .unwrap();
        // Em modo add_new um agente renomeado (manager → manager-2) deve
        // ficar registrado pelo nome final, senão `_meta.agent_origin/manager-2`
        // some e o "manager" original (que veio de outro pack) seria
        // sobrescrito incorretamente.
        for (_src_name, final_name) in &final_names {
            let mut entry = Map::new();
            entry.insert("category".into(), Value::String(seed_category.to_string()));
            entry.insert("slug".into(), Value::String(seed_slug.to_string()));
            origin.insert(final_name.clone(), Value::Object(entry));
        }
    }

    let manifest_json = serde_json::to_string_pretty(&target)?;
    fs::write(&target_manifest_path, format!("{manifest_json}\n"))?;

    // Best-practices — pack-level + agent-level, dedup preservando ordem.
    let pack_meta = read_meta(&pack_ref.dir);
    let mut practice_ids: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let Some(arr) = pack_meta.get("best_practices").and_then(Value::as_array) {
        for v in arr {
            if let Some(s) = v.as_str() {
                if seen.insert(s.to_string()) {
                    practice_ids.push(s.to_string());
                }
            }
        }
    }
    for cfg in src_agents.values() {
        if let Some(arr) = cfg.get("best_practices").and_then(Value::as_array) {
            for v in arr {
                if let Some(s) = v.as_str() {
                    if seen.insert(s.to_string()) {
                        practice_ids.push(s.to_string());
                    }
                }
            }
        }
    }
    let practices = if practice_ids.is_empty() {
        ApplyPracticesResult {
            copied: Vec::new(),
            preserved: Vec::new(),
            missing: Vec::new(),
        }
    } else {
        let refs: Vec<&str> = practice_ids.iter().map(String::as_str).collect();
        apply_practices(bp_dir, &mem_dir, &refs, overwrite)?
    };

    let skills_report = if let Some(filter) = skills {
        let names_opt: Option<Vec<String>> = match &filter {
            SkillsFilter::All => None,
            SkillsFilter::Only(v) => Some(v.clone()),
        };
        let names_slice: Option<&[String]> = names_opt.as_deref();
        apply_skills(SkillsInput {
            seeds_dir,
            category: &pack_ref.category,
            pack: &pack_ref.pack,
            names: names_slice,
            mem_dir: &mem_dir,
            project_root,
            overwrite,
        })
        .map_err(|e| {
            Error::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        })?
    } else {
        SkillsResult::default()
    };

    let ide_report = if let Some(ide_name) = ide {
        Some(ide_apply(ide_name, &mem_dir, project_root)?)
    } else {
        None
    };

    Ok(ApplyResult {
        added,
        replaced,
        skipped,
        md_copied,
        md_skipped,
        mem_dir,
        practices,
        skills: skills_report,
        ide: ide_report,
    })
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn write(p: &Path, content: &str) {
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(p, content).unwrap();
    }

    fn seed_min(tmp: &Path) {
        // Category meta
        write(
            &tmp.join("seeds/dev/meta.json"),
            r#"{"label": "Dev", "description": "Engenharia"}"#,
        );
        // Pack with 2 agents
        let manifest = json!({
            "manager": "manager",
            "oracle": "oracle",
            "rule": "rule-x",
            "agents": {
                "coder": { "name": "coder", "model": "sonnet" },
                "reviewer": { "name": "reviewer", "model": "opus" }
            }
        });
        write(
            &tmp.join("seeds/dev/base/agents.json"),
            &serde_json::to_string_pretty(&manifest).unwrap(),
        );
        write(&tmp.join("seeds/dev/base/agents/coder.md"), "# coder");
        write(&tmp.join("seeds/dev/base/agents/reviewer.md"), "# reviewer");
        write(
            &tmp.join("seeds/dev/base/meta.json"),
            r#"{"label": "Base"}"#,
        );
    }

    // ── read_meta ────────────────────────────────────────────────────────

    #[test]
    fn read_meta_returns_empty_on_missing() {
        let tmp = tempdir().unwrap();
        assert!(read_meta(tmp.path()).is_empty());
    }

    #[test]
    fn read_meta_parses_json() {
        let tmp = tempdir().unwrap();
        write(&tmp.path().join("meta.json"), r#"{"label": "x"}"#);
        let m = read_meta(tmp.path());
        assert_eq!(m.get("label").unwrap(), &json!("x"));
    }

    #[test]
    fn read_meta_returns_empty_on_malformed() {
        let tmp = tempdir().unwrap();
        write(&tmp.path().join("meta.json"), "not json");
        assert!(read_meta(tmp.path()).is_empty());
    }

    // ── list_categories ─────────────────────────────────────────────────

    #[test]
    fn list_categories_returns_empty_when_missing() {
        let tmp = tempdir().unwrap();
        let seeds = tmp.path().join("seeds");
        assert!(list_categories(&seeds).is_empty());
    }

    #[test]
    fn list_categories_includes_meta() {
        let tmp = tempdir().unwrap();
        seed_min(tmp.path());
        let cats = list_categories(&tmp.path().join("seeds"));
        assert_eq!(cats.len(), 1);
        assert_eq!(cats[0].name, "dev");
        assert_eq!(cats[0].meta.get("label").unwrap(), &json!("Dev"));
    }

    // ── list_packs ──────────────────────────────────────────────────────

    #[test]
    fn list_packs_returns_empty_when_missing() {
        let tmp = tempdir().unwrap();
        assert!(list_packs(&tmp.path().join("seeds"), "dev").is_empty());
    }

    #[test]
    fn list_packs_discovers() {
        let tmp = tempdir().unwrap();
        seed_min(tmp.path());
        let packs = list_packs(&tmp.path().join("seeds"), "dev");
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].name, "base");
        assert_eq!(
            packs[0].agent_names,
            vec!["coder".to_string(), "reviewer".to_string()]
        );
        assert_eq!(packs[0].meta.get("label").unwrap(), &json!("Base"));
    }

    #[test]
    fn list_packs_skips_dirs_without_manifest() {
        let tmp = tempdir().unwrap();
        seed_min(tmp.path());
        fs::create_dir_all(tmp.path().join("seeds/dev/empty")).unwrap();
        let packs = list_packs(&tmp.path().join("seeds"), "dev");
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].name, "base");
    }

    // ── load_pack_ref ───────────────────────────────────────────────────

    #[test]
    fn load_pack_ref_errors_on_invalid_ref() {
        let tmp = tempdir().unwrap();
        let seeds = tmp.path().join("seeds");
        fs::create_dir_all(&seeds).unwrap();
        assert!(matches!(
            load_pack_ref(&seeds, "invalid"),
            Err(Error::InvalidRef(_))
        ));
        assert!(matches!(
            load_pack_ref(&seeds, ""),
            Err(Error::InvalidRef(_))
        ));
        assert!(matches!(
            load_pack_ref(&seeds, "cat/"),
            Err(Error::InvalidRef(_))
        ));
    }

    #[test]
    fn load_pack_ref_errors_on_missing_pack() {
        let tmp = tempdir().unwrap();
        let seeds = tmp.path().join("seeds");
        fs::create_dir_all(&seeds).unwrap();
        assert!(matches!(
            load_pack_ref(&seeds, "x/y"),
            Err(Error::PackMissing(_, _))
        ));
    }

    #[test]
    fn load_pack_ref_loads_manifest() {
        let tmp = tempdir().unwrap();
        seed_min(tmp.path());
        let pr = load_pack_ref(&tmp.path().join("seeds"), "dev/base").unwrap();
        assert_eq!(pr.category, "dev");
        assert_eq!(pr.pack, "base");
        assert!(pr.manifest.get("agents").is_some());
    }

    // ── filter_agents ───────────────────────────────────────────────────

    #[test]
    fn filter_agents_passthrough_when_no_filters() {
        let agents = json!({"a": {}, "b": {}, "c": {}})
            .as_object()
            .unwrap()
            .clone();
        let out = filter_agents(&agents, None, None);
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn filter_agents_only_keeps_listed() {
        let agents = json!({"a": {}, "b": {}, "c": {}})
            .as_object()
            .unwrap()
            .clone();
        let out = filter_agents(&agents, Some("a,c"), None);
        assert_eq!(out.len(), 2);
        assert!(out.contains_key("a"));
        assert!(out.contains_key("c"));
    }

    #[test]
    fn filter_agents_exclude_removes_listed() {
        let agents = json!({"a": {}, "b": {}, "c": {}})
            .as_object()
            .unwrap()
            .clone();
        let out = filter_agents(&agents, None, Some("b"));
        assert_eq!(out.len(), 2);
        assert!(!out.contains_key("b"));
    }

    #[test]
    fn filter_agents_only_and_exclude_intersect() {
        let agents = json!({"a": {}, "b": {}, "c": {}})
            .as_object()
            .unwrap()
            .clone();
        let out = filter_agents(&agents, Some("a,b"), Some("b"));
        assert_eq!(out.len(), 1);
        assert!(out.contains_key("a"));
    }

    // ── apply ───────────────────────────────────────────────────────────

    fn setup_bp(tmp: &Path) -> PathBuf {
        let bp = tmp.join("best-practices");
        fs::create_dir_all(&bp).unwrap();
        bp
    }

    #[test]
    fn apply_copies_agents_and_contexts() {
        let tmp = tempdir().unwrap();
        seed_min(tmp.path());
        let bp = setup_bp(tmp.path());
        let proj = tmp.path().join("proj");
        fs::create_dir_all(&proj).unwrap();

        let r = apply(ApplyInput {
            seeds_dir: &tmp.path().join("seeds"),
            bp_dir: &bp,
            reference: "dev/base",
            project_root: &proj,
            mem_dir_name: ".poupar",
            only: None,
            exclude: None,
            overwrite: false,
            add_new: false,
            skills: None,
            ide: None,
        })
        .unwrap();

        assert_eq!(r.added, vec!["coder".to_string(), "reviewer".to_string()]);
        assert!(r.skipped.is_empty());
        assert_eq!(r.md_copied.len(), 2);
        let manifest: Value = serde_json::from_str(
            &fs::read_to_string(proj.join(".poupar/agents.json")).unwrap(),
        )
        .unwrap();
        assert!(manifest.pointer("/agents/coder").is_some());
        assert!(manifest.pointer("/agents/reviewer").is_some());
        assert_eq!(
            manifest
                .pointer("/_meta/agent_origin/coder/category")
                .unwrap(),
            &json!("dev")
        );
        assert_eq!(
            manifest.pointer("/_meta/agent_origin/coder/slug").unwrap(),
            &json!("base")
        );
    }

    #[test]
    fn apply_skips_existing_agents_without_overwrite() {
        let tmp = tempdir().unwrap();
        seed_min(tmp.path());
        let bp = setup_bp(tmp.path());
        let proj = tmp.path().join("proj");
        fs::create_dir_all(&proj).unwrap();

        let _ = apply(ApplyInput {
            seeds_dir: &tmp.path().join("seeds"),
            bp_dir: &bp,
            reference: "dev/base",
            project_root: &proj,
            mem_dir_name: ".poupar",
            only: None,
            exclude: None,
            overwrite: false,
            add_new: false,
            skills: None,
            ide: None,
        })
        .unwrap();

        let r = apply(ApplyInput {
            seeds_dir: &tmp.path().join("seeds"),
            bp_dir: &bp,
            reference: "dev/base",
            project_root: &proj,
            mem_dir_name: ".poupar",
            only: None,
            exclude: None,
            overwrite: false,
            add_new: false,
            skills: None,
            ide: None,
        })
        .unwrap();
        assert!(r.added.is_empty());
        assert_eq!(r.skipped.len(), 2);
        assert!(r.md_skipped.len() >= 2);
    }

    #[test]
    fn apply_replaces_with_overwrite() {
        let tmp = tempdir().unwrap();
        seed_min(tmp.path());
        let bp = setup_bp(tmp.path());
        let proj = tmp.path().join("proj");
        fs::create_dir_all(&proj).unwrap();

        let _ = apply(ApplyInput {
            seeds_dir: &tmp.path().join("seeds"),
            bp_dir: &bp,
            reference: "dev/base",
            project_root: &proj,
            mem_dir_name: ".poupar",
            only: None,
            exclude: None,
            overwrite: false,
            add_new: false,
            skills: None,
            ide: None,
        })
        .unwrap();

        let r = apply(ApplyInput {
            seeds_dir: &tmp.path().join("seeds"),
            bp_dir: &bp,
            reference: "dev/base",
            project_root: &proj,
            mem_dir_name: ".poupar",
            only: None,
            exclude: None,
            overwrite: true,
            add_new: false,
            skills: None,
            ide: None,
        })
        .unwrap();
        assert_eq!(r.replaced.len(), 2);
        assert!(r.skipped.is_empty());
    }

    #[test]
    fn apply_add_new_suffixes_colliding_agents() {
        let tmp = tempdir().unwrap();
        seed_min(tmp.path());
        let bp = setup_bp(tmp.path());
        let proj = tmp.path().join("proj");
        fs::create_dir_all(&proj).unwrap();

        // 1ª instalação: coder + reviewer instalados pelo nome original.
        let _ = apply(ApplyInput {
            seeds_dir: &tmp.path().join("seeds"),
            bp_dir: &bp,
            reference: "dev/base",
            project_root: &proj,
            mem_dir_name: ".poupar",
            only: None,
            exclude: None,
            overwrite: false,
            add_new: false,
            skills: None,
            ide: None,
        })
        .unwrap();

        // 2ª instalação com add_new: deve suffixar pra coder-2, reviewer-2.
        let r = apply(ApplyInput {
            seeds_dir: &tmp.path().join("seeds"),
            bp_dir: &bp,
            reference: "dev/base",
            project_root: &proj,
            mem_dir_name: ".poupar",
            only: None,
            exclude: None,
            overwrite: false,
            add_new: true,
            skills: None,
            ide: None,
        })
        .unwrap();
        assert!(r.skipped.is_empty(), "skipped should be empty in add_new mode: {:?}", r.skipped);
        assert!(r.replaced.is_empty(), "replaced should be empty: {:?}", r.replaced);
        assert_eq!(r.added.len(), 2);
        assert!(r.added.contains(&"coder-2".to_string()));
        assert!(r.added.contains(&"reviewer-2".to_string()));

        // 3ª: deve ir pra -3.
        let r = apply(ApplyInput {
            seeds_dir: &tmp.path().join("seeds"),
            bp_dir: &bp,
            reference: "dev/base",
            project_root: &proj,
            mem_dir_name: ".poupar",
            only: None,
            exclude: None,
            overwrite: false,
            add_new: true,
            skills: None,
            ide: None,
        })
        .unwrap();
        assert!(r.added.contains(&"coder-3".to_string()));
        assert!(r.added.contains(&"reviewer-3".to_string()));

        // Verifica manifest no disco: context_file aponta pro .md certo,
        // agent_origin tem os nomes finais.
        let manifest: Value = serde_json::from_str(
            &fs::read_to_string(proj.join(".poupar/agents.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            manifest.pointer("/agents/coder-2/context_file").and_then(Value::as_str),
            Some("agents/coder-2.md"),
        );
        assert!(manifest.pointer("/_meta/agent_origin/coder-2").is_some());
        assert!(manifest.pointer("/_meta/agent_origin/coder-3").is_some());

        // .md files devem existir com nomes suffixados.
        assert!(proj.join(".poupar/agents/coder-2.md").exists());
        assert!(proj.join(".poupar/agents/reviewer-3.md").exists());
    }

    #[test]
    fn apply_with_only_filter() {
        let tmp = tempdir().unwrap();
        seed_min(tmp.path());
        let bp = setup_bp(tmp.path());
        let proj = tmp.path().join("proj");
        fs::create_dir_all(&proj).unwrap();

        let r = apply(ApplyInput {
            seeds_dir: &tmp.path().join("seeds"),
            bp_dir: &bp,
            reference: "dev/base",
            project_root: &proj,
            mem_dir_name: ".poupar",
            only: Some("coder"),
            exclude: None,
            overwrite: false,
            add_new: false,
            skills: None,
            ide: None,
        })
        .unwrap();
        assert_eq!(r.added, vec!["coder".to_string()]);
    }

    #[test]
    fn apply_creates_mem_dir_structure() {
        let tmp = tempdir().unwrap();
        seed_min(tmp.path());
        let bp = setup_bp(tmp.path());
        let proj = tmp.path().join("proj");
        fs::create_dir_all(&proj).unwrap();

        let _ = apply(ApplyInput {
            seeds_dir: &tmp.path().join("seeds"),
            bp_dir: &bp,
            reference: "dev/base",
            project_root: &proj,
            mem_dir_name: ".poupar",
            only: None,
            exclude: None,
            overwrite: false,
            add_new: false,
            skills: None,
            ide: None,
        })
        .unwrap();
        assert!(proj.join(".poupar").is_dir());
        assert!(proj.join(".poupar/agents").is_dir());
        assert!(proj.join(".poupar/agents.json").is_file());
    }

    #[test]
    fn apply_errors_on_missing_pack() {
        let tmp = tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("seeds")).unwrap();
        let bp = setup_bp(tmp.path());
        let proj = tmp.path().join("proj");
        fs::create_dir_all(&proj).unwrap();

        let err = apply(ApplyInput {
            seeds_dir: &tmp.path().join("seeds"),
            bp_dir: &bp,
            reference: "x/y",
            project_root: &proj,
            mem_dir_name: ".poupar",
            only: None,
            exclude: None,
            overwrite: false,
            add_new: false,
            skills: None,
            ide: None,
        })
        .unwrap_err();
        assert!(matches!(err, Error::PackMissing(_, _)));
    }
}
