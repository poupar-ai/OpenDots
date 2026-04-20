//! Skills — integrações instaláveis com MCP opcional.
//!
//! Porta de `plugin/scripts/skills-install.cjs`. `SEEDS_DIR` era derivado de
//! `__dirname/../seeds` no Node; aqui vira parâmetro `seeds_dir: &Path`.

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

// ── Frontmatter ──────────────────────────────────────────────────────────────

fn fm_block_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?s)^---\n(.*?)\n---").unwrap())
}

fn fm_line_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(\w+):\s*(.*)$").unwrap())
}

fn fm_array_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\[(.*)\]$").unwrap())
}

fn fm_dquoted_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"^".*"$"#).unwrap())
}

fn fm_squoted_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^'.*'$").unwrap())
}

/// Emula `parseFrontmatter` do `.cjs`. Retorna objeto vazio se não há bloco
/// `---\n...\n---` no topo.
pub fn parse_frontmatter(md: &str) -> Map<String, Value> {
    let mut out = Map::new();
    let Some(cap) = fm_block_re().captures(md) else {
        return out;
    };
    let body = cap.get(1).unwrap().as_str();
    for line in body.split('\n') {
        let Some(kv) = fm_line_re().captures(line) else {
            continue;
        };
        let k = kv.get(1).unwrap().as_str().to_string();
        let v = kv.get(2).unwrap().as_str();
        let value = if v == "true" {
            Value::Bool(true)
        } else if v == "false" {
            Value::Bool(false)
        } else if fm_array_re().is_match(v) {
            let inner = &v[1..v.len() - 1];
            let items: Vec<Value> = inner
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| Value::String(s.to_string()))
                .collect();
            Value::Array(items)
        } else if fm_dquoted_re().is_match(v) || fm_squoted_re().is_match(v) {
            Value::String(v[1..v.len() - 1].to_string())
        } else {
            Value::String(v.to_string())
        };
        out.insert(k, value);
    }
    out
}

// ── Skill ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub scope: String,
    pub version: String,
    pub requires_mcp: bool,
    pub env: Vec<String>,
    pub has_mcp: bool,
}

fn fm_str(fm: &Map<String, Value>, key: &str) -> String {
    match fm.get(key) {
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    }
}

fn fm_bool(fm: &Map<String, Value>, key: &str) -> bool {
    matches!(fm.get(key), Some(Value::Bool(true)))
}

fn fm_array(fm: &Map<String, Value>, key: &str) -> Vec<String> {
    match fm.get(key) {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        _ => Vec::new(),
    }
}

pub fn list_skills_in_pack(seeds_dir: &Path, category: &str, pack: &str) -> Vec<Skill> {
    let dir = seeds_dir.join(category).join(pack).join("skills");
    if !dir.exists() {
        return Vec::new();
    }
    let mut entries: Vec<_> = match fs::read_dir(&dir) {
        Ok(it) => it.flatten().collect(),
        Err(_) => return Vec::new(),
    };
    // readdirSync do Node retorna em ordem do FS (depende da plataforma).
    // No Linux é ordem de criação; para parity estável, ordenamos por nome.
    entries.sort_by_key(|e| e.file_name());
    entries
        .into_iter()
        .filter_map(|entry| {
            let p = entry.path();
            if !p.is_dir() {
                return None;
            }
            let skill_md = p.join("SKILL.md");
            if !skill_md.exists() {
                return None;
            }
            let md = fs::read_to_string(&skill_md).ok()?;
            let mcp_path = p.join("mcp.json");
            let fm = parse_frontmatter(&md);
            Some(Skill {
                name: entry.file_name().to_string_lossy().into_owned(),
                description: fm_str(&fm, "description"),
                scope: fm_str(&fm, "scope"),
                version: fm_str(&fm, "version"),
                requires_mcp: fm_bool(&fm, "requires_mcp"),
                env: fm_array(&fm, "env"),
                has_mcp: mcp_path.exists(),
            })
        })
        .collect()
}

// ── Apply ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ApplyResult {
    pub copied: Vec<String>,
    pub preserved: Vec<String>,
    pub mcp_added: Vec<String>,
    pub mcp_preserved: Vec<String>,
}

pub struct ApplyInput<'a> {
    pub seeds_dir: &'a Path,
    pub category: &'a str,
    pub pack: &'a str,
    /// `None` = instalar todas; `Some([])` também pega nenhuma (emula `names &&
    /// names.length` do `.cjs`). Para replicar exato: passar `None` aplica todas.
    pub names: Option<&'a [String]>,
    pub mem_dir: &'a Path,
    pub project_root: &'a Path,
    pub overwrite: bool,
}

pub fn apply_skills(input: ApplyInput) -> Result<ApplyResult, Error> {
    let ApplyInput {
        seeds_dir,
        category,
        pack,
        names,
        mem_dir,
        project_root,
        overwrite,
    } = input;

    let src_dir = seeds_dir.join(category).join(pack).join("skills");
    if !src_dir.exists() {
        return Ok(ApplyResult::default());
    }

    let out_dir = mem_dir.join("skills");
    if !out_dir.exists() {
        fs::create_dir_all(&out_dir)?;
    }

    let all = list_skills_in_pack(seeds_dir, category, pack);
    // `names && names.length` no .cjs → só filtra se há lista não-vazia.
    let pick: Vec<Skill> = match names {
        Some(list) if !list.is_empty() => all
            .into_iter()
            .filter(|s| list.iter().any(|n| n == &s.name))
            .collect(),
        _ => all,
    };

    let mcp_project_path = project_root.join(".mcp.json");
    let mut mcp_root: Value = if mcp_project_path.exists() {
        match fs::read_to_string(&mcp_project_path).ok().and_then(|s| {
            let parsed: Option<Value> = serde_json::from_str(&s).ok();
            parsed
        }) {
            Some(v) => v,
            None => Value::Object({
                let mut m = Map::new();
                m.insert("mcpServers".into(), Value::Object(Map::new()));
                m
            }),
        }
    } else {
        let mut m = Map::new();
        m.insert("mcpServers".into(), Value::Object(Map::new()));
        Value::Object(m)
    };
    // Garante mcpServers mesmo se JSON carregado não tinha a chave.
    if let Value::Object(ref mut obj) = mcp_root {
        if !obj.contains_key("mcpServers") {
            obj.insert("mcpServers".into(), Value::Object(Map::new()));
        }
    } else {
        // Se root não era objeto (improvável), normaliza.
        let mut m = Map::new();
        m.insert("mcpServers".into(), Value::Object(Map::new()));
        mcp_root = Value::Object(m);
    }

    let mut copied = Vec::new();
    let mut preserved = Vec::new();
    let mut mcp_added = Vec::new();
    let mut mcp_preserved = Vec::new();

    for skill in &pick {
        let s_src = src_dir.join(&skill.name).join("SKILL.md");
        let s_dst_dir = out_dir.join(&skill.name);
        if !s_dst_dir.exists() {
            fs::create_dir_all(&s_dst_dir)?;
        }
        let s_dst = s_dst_dir.join("SKILL.md");
        if s_dst.exists() && !overwrite {
            preserved.push(skill.name.clone());
        } else {
            fs::copy(&s_src, &s_dst)?;
            copied.push(skill.name.clone());
        }

        let mcp_src = src_dir.join(&skill.name).join("mcp.json");
        if mcp_src.exists() {
            fs::copy(&mcp_src, s_dst_dir.join("mcp.json"))?;
            if let Ok(raw) = fs::read_to_string(&mcp_src) {
                if let Ok(entry) = serde_json::from_str::<Value>(&raw) {
                    if let Value::Object(ref mut obj) = mcp_root {
                        if let Some(Value::Object(ref mut servers)) = obj.get_mut("mcpServers") {
                            if servers.contains_key(&skill.name) && !overwrite {
                                mcp_preserved.push(skill.name.clone());
                            } else {
                                servers.insert(skill.name.clone(), entry);
                                mcp_added.push(skill.name.clone());
                            }
                        }
                    }
                }
            }
        }
    }

    if !mcp_added.is_empty() {
        let serialized = serde_json::to_string_pretty(&mcp_root)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        fs::write(&mcp_project_path, format!("{serialized}\n"))?;
    }

    Ok(ApplyResult {
        copied,
        preserved,
        mcp_added,
        mcp_preserved,
    })
}

// Re-export para quem queira estender (mantém API coesa)
pub fn seeds_skill_dir(seeds_dir: &Path, category: &str, pack: &str) -> PathBuf {
    seeds_dir.join(category).join(pack).join("skills")
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

    // ── parse_frontmatter ────────────────────────────────────────────────

    #[test]
    fn parse_frontmatter_empty_when_no_block() {
        assert!(parse_frontmatter("no frontmatter here").is_empty());
    }

    #[test]
    fn parse_frontmatter_parses_string_scalar() {
        let fm = parse_frontmatter("---\ndescription: Hello world\n---\nbody");
        assert_eq!(fm.get("description").unwrap(), &json!("Hello world"));
    }

    #[test]
    fn parse_frontmatter_parses_bools() {
        let fm = parse_frontmatter("---\nrequires_mcp: true\nactive: false\n---");
        assert_eq!(fm.get("requires_mcp").unwrap(), &json!(true));
        assert_eq!(fm.get("active").unwrap(), &json!(false));
    }

    #[test]
    fn parse_frontmatter_parses_inline_array() {
        let fm = parse_frontmatter("---\nenv: [FOO, BAR, BAZ]\n---");
        assert_eq!(fm.get("env").unwrap(), &json!(["FOO", "BAR", "BAZ"]));
    }

    #[test]
    fn parse_frontmatter_parses_empty_array() {
        let fm = parse_frontmatter("---\nenv: []\n---");
        assert_eq!(fm.get("env").unwrap(), &json!([]));
    }

    #[test]
    fn parse_frontmatter_strips_double_and_single_quotes() {
        let fm = parse_frontmatter("---\na: \"hello\"\nb: 'world'\n---");
        assert_eq!(fm.get("a").unwrap(), &json!("hello"));
        assert_eq!(fm.get("b").unwrap(), &json!("world"));
    }

    #[test]
    fn parse_frontmatter_ignores_unparseable_lines() {
        let fm = parse_frontmatter("---\nvalid: yes\n  weird indent\n---");
        assert_eq!(fm.len(), 1);
    }

    #[test]
    fn parse_frontmatter_ignores_hyphen_keys() {
        // `\w+` do .cjs não inclui hífen
        let fm = parse_frontmatter("---\nskill-name: foo\nname: bar\n---");
        assert!(!fm.contains_key("skill-name"));
        assert_eq!(fm.get("name").unwrap(), &json!("bar"));
    }

    // ── list_skills_in_pack ──────────────────────────────────────────────

    #[test]
    fn list_skills_returns_empty_when_missing_dir() {
        let tmp = tempdir().unwrap();
        assert!(list_skills_in_pack(tmp.path(), "cat", "pack").is_empty());
    }

    #[test]
    fn list_skills_discovers_skill_md() {
        let tmp = tempdir().unwrap();
        write(
            &tmp.path().join("cat/pack/skills/alpha/SKILL.md"),
            "---\ndescription: Alpha\nrequires_mcp: true\nenv: [A, B]\n---\nbody",
        );
        let list = list_skills_in_pack(tmp.path(), "cat", "pack");
        assert_eq!(list.len(), 1);
        let s = &list[0];
        assert_eq!(s.name, "alpha");
        assert_eq!(s.description, "Alpha");
        assert!(s.requires_mcp);
        assert_eq!(s.env, vec!["A", "B"]);
        assert!(!s.has_mcp);
    }

    #[test]
    fn list_skills_marks_has_mcp_when_file_exists() {
        let tmp = tempdir().unwrap();
        write(
            &tmp.path().join("cat/pack/skills/s1/SKILL.md"),
            "---\ndescription: x\n---",
        );
        write(
            &tmp.path().join("cat/pack/skills/s1/mcp.json"),
            "{\"command\":\"x\"}",
        );
        let list = list_skills_in_pack(tmp.path(), "cat", "pack");
        assert!(list[0].has_mcp);
    }

    #[test]
    fn list_skills_skips_dirs_without_skill_md() {
        let tmp = tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("cat/pack/skills/empty")).unwrap();
        write(
            &tmp.path().join("cat/pack/skills/valid/SKILL.md"),
            "---\n---",
        );
        let list = list_skills_in_pack(tmp.path(), "cat", "pack");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "valid");
    }

    // ── apply_skills ─────────────────────────────────────────────────────

    fn seed_pack(tmp: &Path) {
        write(
            &tmp.join("seeds/cat/pack/skills/s1/SKILL.md"),
            "---\ndescription: Skill 1\n---\nbody1",
        );
        write(
            &tmp.join("seeds/cat/pack/skills/s1/mcp.json"),
            r#"{"command": "node", "args": ["s1.js"]}"#,
        );
        write(
            &tmp.join("seeds/cat/pack/skills/s2/SKILL.md"),
            "---\ndescription: Skill 2\n---\nbody2",
        );
    }

    #[test]
    fn apply_copies_skill_md_and_merges_mcp() {
        let tmp = tempdir().unwrap();
        seed_pack(tmp.path());
        let mem = tmp.path().join("mem");
        let proj = tmp.path().join("proj");
        fs::create_dir_all(&proj).unwrap();
        let r = apply_skills(ApplyInput {
            seeds_dir: &tmp.path().join("seeds"),
            category: "cat",
            pack: "pack",
            names: None,
            mem_dir: &mem,
            project_root: &proj,
            overwrite: false,
        })
        .unwrap();
        assert_eq!(r.copied, vec!["s1".to_string(), "s2".to_string()]);
        assert!(r.preserved.is_empty());
        assert_eq!(r.mcp_added, vec!["s1".to_string()]);
        assert!(r.mcp_preserved.is_empty());
        // SKILL.md copiado
        assert!(mem.join("skills/s1/SKILL.md").exists());
        assert!(mem.join("skills/s2/SKILL.md").exists());
        // .mcp.json criado e inclui s1
        let mcp: Value =
            serde_json::from_str(&fs::read_to_string(proj.join(".mcp.json")).unwrap()).unwrap();
        assert!(mcp
            .pointer("/mcpServers/s1/command")
            .map(|v| v == "node")
            .unwrap_or(false));
    }

    #[test]
    fn apply_preserves_existing_mcp_entry_without_overwrite() {
        let tmp = tempdir().unwrap();
        seed_pack(tmp.path());
        let mem = tmp.path().join("mem");
        let proj = tmp.path().join("proj");
        fs::create_dir_all(&proj).unwrap();
        write(
            &proj.join(".mcp.json"),
            r#"{"mcpServers":{"s1":{"command":"existing"}}}"#,
        );
        let r = apply_skills(ApplyInput {
            seeds_dir: &tmp.path().join("seeds"),
            category: "cat",
            pack: "pack",
            names: None,
            mem_dir: &mem,
            project_root: &proj,
            overwrite: false,
        })
        .unwrap();
        assert!(r.mcp_preserved.contains(&"s1".to_string()));
        assert!(!r.mcp_added.contains(&"s1".to_string()));
        // conteúdo preservado
        let mcp: Value =
            serde_json::from_str(&fs::read_to_string(proj.join(".mcp.json")).unwrap()).unwrap();
        assert_eq!(
            mcp.pointer("/mcpServers/s1/command").unwrap().as_str(),
            Some("existing")
        );
    }

    #[test]
    fn apply_overwrites_mcp_when_flag_set() {
        let tmp = tempdir().unwrap();
        seed_pack(tmp.path());
        let mem = tmp.path().join("mem");
        let proj = tmp.path().join("proj");
        fs::create_dir_all(&proj).unwrap();
        write(
            &proj.join(".mcp.json"),
            r#"{"mcpServers":{"s1":{"command":"existing"}}}"#,
        );
        let r = apply_skills(ApplyInput {
            seeds_dir: &tmp.path().join("seeds"),
            category: "cat",
            pack: "pack",
            names: None,
            mem_dir: &mem,
            project_root: &proj,
            overwrite: true,
        })
        .unwrap();
        assert!(r.mcp_added.contains(&"s1".to_string()));
        let mcp: Value =
            serde_json::from_str(&fs::read_to_string(proj.join(".mcp.json")).unwrap()).unwrap();
        assert_eq!(
            mcp.pointer("/mcpServers/s1/command").unwrap().as_str(),
            Some("node")
        );
    }

    #[test]
    fn apply_preserves_existing_skill_md_without_overwrite() {
        let tmp = tempdir().unwrap();
        seed_pack(tmp.path());
        let mem = tmp.path().join("mem");
        let proj = tmp.path().join("proj");
        fs::create_dir_all(&proj).unwrap();
        write(&mem.join("skills/s1/SKILL.md"), "custom content");
        let r = apply_skills(ApplyInput {
            seeds_dir: &tmp.path().join("seeds"),
            category: "cat",
            pack: "pack",
            names: None,
            mem_dir: &mem,
            project_root: &proj,
            overwrite: false,
        })
        .unwrap();
        assert!(r.preserved.contains(&"s1".to_string()));
        assert!(r.copied.contains(&"s2".to_string()));
        assert_eq!(
            fs::read_to_string(mem.join("skills/s1/SKILL.md")).unwrap(),
            "custom content"
        );
    }

    #[test]
    fn apply_filters_by_names() {
        let tmp = tempdir().unwrap();
        seed_pack(tmp.path());
        let mem = tmp.path().join("mem");
        let proj = tmp.path().join("proj");
        fs::create_dir_all(&proj).unwrap();
        let r = apply_skills(ApplyInput {
            seeds_dir: &tmp.path().join("seeds"),
            category: "cat",
            pack: "pack",
            names: Some(&["s2".to_string()]),
            mem_dir: &mem,
            project_root: &proj,
            overwrite: false,
        })
        .unwrap();
        assert_eq!(r.copied, vec!["s2".to_string()]);
        assert!(!mem.join("skills/s1").exists());
    }

    #[test]
    fn apply_empty_names_installs_all_like_cjs() {
        // .cjs usa `names && names.length`, então `[]` cai pro else (todos).
        let tmp = tempdir().unwrap();
        seed_pack(tmp.path());
        let mem = tmp.path().join("mem");
        let proj = tmp.path().join("proj");
        fs::create_dir_all(&proj).unwrap();
        let empty: [String; 0] = [];
        let r = apply_skills(ApplyInput {
            seeds_dir: &tmp.path().join("seeds"),
            category: "cat",
            pack: "pack",
            names: Some(&empty),
            mem_dir: &mem,
            project_root: &proj,
            overwrite: false,
        })
        .unwrap();
        assert_eq!(r.copied.len(), 2);
    }

    #[test]
    fn apply_no_op_when_source_dir_missing() {
        let tmp = tempdir().unwrap();
        let mem = tmp.path().join("mem");
        let proj = tmp.path().join("proj");
        fs::create_dir_all(&proj).unwrap();
        let r = apply_skills(ApplyInput {
            seeds_dir: &tmp.path().join("seeds"),
            category: "cat",
            pack: "pack",
            names: None,
            mem_dir: &mem,
            project_root: &proj,
            overwrite: false,
        })
        .unwrap();
        assert_eq!(r, ApplyResult::default());
    }

    #[test]
    fn apply_does_not_create_mcp_file_when_no_mcp_added() {
        let tmp = tempdir().unwrap();
        write(
            &tmp.path().join("seeds/cat/pack/skills/s1/SKILL.md"),
            "---\ndescription: x\n---",
        );
        let mem = tmp.path().join("mem");
        let proj = tmp.path().join("proj");
        fs::create_dir_all(&proj).unwrap();
        let r = apply_skills(ApplyInput {
            seeds_dir: &tmp.path().join("seeds"),
            category: "cat",
            pack: "pack",
            names: None,
            mem_dir: &mem,
            project_root: &proj,
            overwrite: false,
        })
        .unwrap();
        assert!(r.mcp_added.is_empty());
        assert!(!proj.join(".mcp.json").exists());
    }
}
