//! # domain-ide-adapters
//!
//! Porta Rust de `plugin/scripts/ide-adapters.cjs` — gera arquivos
//! de configuração por IDE a partir do manifest canônico
//! `agents.json` (fonte única de verdade).

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const SUPPORTED: &[&str] = &["claude-code", "cursor", "vscode-copilot", "codex"];

#[derive(Debug, Error)]
pub enum IdeError {
    #[error("IDE não suportada: {0}. Use: {supported}", supported = SUPPORTED.join(", "))]
    Unsupported(String),
    #[error("agents.json não encontrado em {0}")]
    ManifestMissing(PathBuf),
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ide {
    ClaudeCode,
    Cursor,
    VscodeCopilot,
    Codex,
}

impl Ide {
    pub fn parse(s: &str) -> Result<Self, IdeError> {
        match s {
            "claude-code" => Ok(Self::ClaudeCode),
            "cursor" => Ok(Self::Cursor),
            "vscode-copilot" => Ok(Self::VscodeCopilot),
            "codex" => Ok(Self::Codex),
            _ => Err(IdeError::Unsupported(s.into())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyResult {
    /// Paths relativos ao `project_root`, na ordem de escrita do `.cjs`.
    pub written: Vec<String>,
}

fn read_manifest(mem_dir: &Path) -> Result<Value, IdeError> {
    let f = mem_dir.join("agents.json");
    let raw = fs::read_to_string(&f)
        .map_err(|_| IdeError::ManifestMissing(mem_dir.to_path_buf()))?;
    serde_json::from_str(&raw).map_err(|_| IdeError::ManifestMissing(mem_dir.to_path_buf()))
}

fn read_context(mem_dir: &Path, context_file: Option<&str>) -> String {
    let Some(ctx) = context_file else {
        return String::new();
    };
    let f = mem_dir.join(ctx);
    fs::read_to_string(f).unwrap_or_default()
}

fn ensure_dir(dir: &Path) -> std::io::Result<()> {
    if !dir.exists() {
        fs::create_dir_all(dir)?;
    }
    Ok(())
}

/// Replica JS `str.trim()` (WhiteSpace + LineTerminator do spec).
/// Em Rust, `str::trim` já cobre tudo isso.
fn trim(s: &str) -> &str {
    s.trim()
}

fn path_rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

fn agent_label(cfg: &Value, name: &str) -> String {
    cfg.get("label")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(name)
        .to_string()
}

fn agent_desc_or_label_or_name(cfg: &Value, name: &str) -> String {
    let d = cfg
        .get("description")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    if let Some(s) = d {
        return s.to_string();
    }
    let l = cfg
        .get("label")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    if let Some(s) = l {
        return s.to_string();
    }
    name.to_string()
}

/// Iterate manifest.agents preservando insertion order.
fn agents_iter(manifest: &Value) -> Vec<(String, Value)> {
    manifest
        .get("agents")
        .and_then(Value::as_object)
        .map(|o| {
            o.iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

// ── Adapters ─────────────────────────────────────────────────────────────

pub fn apply_claude_code(
    mem_dir: &Path,
    project_root: &Path,
    manifest: &Value,
) -> Result<ApplyResult, IdeError> {
    let mut written = Vec::new();
    let out_dir = project_root.join(".claude").join("agents");
    ensure_dir(&out_dir)?;
    for (name, cfg) in agents_iter(manifest) {
        let ctx = read_context(
            mem_dir,
            cfg.get("context_file").and_then(Value::as_str),
        );
        let desc = agent_desc_or_label_or_name(&cfg, &name);
        let md = format!(
            "---\nname: {name}\ndescription: {desc}\n---\n\n{ctx}\n",
            ctx = trim(&ctx),
        );
        let out = out_dir.join(format!("{name}.md"));
        fs::write(&out, md)?;
        written.push(path_rel(project_root, &out));
    }
    Ok(ApplyResult { written })
}

pub fn apply_cursor(
    mem_dir: &Path,
    project_root: &Path,
    manifest: &Value,
) -> Result<ApplyResult, IdeError> {
    let mut written = Vec::new();
    let out_dir = project_root.join(".cursor").join("rules");
    ensure_dir(&out_dir)?;
    for (name, cfg) in agents_iter(manifest) {
        let ctx = read_context(mem_dir, cfg.get("context_file").and_then(Value::as_str));
        let desc = agent_desc_or_label_or_name(&cfg, &name);
        let label = agent_label(&cfg, &name);
        let md = format!(
            "---\ndescription: {desc}\nglobs: [\"**/*\"]\nalwaysApply: false\n---\n\n# {label}\n\n{ctx}\n",
            ctx = trim(&ctx),
        );
        let out = out_dir.join(format!("{name}.mdc"));
        fs::write(&out, md)?;
        written.push(path_rel(project_root, &out));
    }
    // Índice .cursorrules
    let mut idx = String::from("# Agentes disponíveis (project-mem)\n\n");
    for (name, cfg) in agents_iter(manifest) {
        let desc = agent_desc_or_label_or_name(&cfg, &name);
        idx.push_str(&format!("- **{name}** — {desc}\n"));
    }
    let idx_path = project_root.join(".cursorrules");
    fs::write(&idx_path, idx)?;
    written.push(".cursorrules".into());
    Ok(ApplyResult { written })
}

pub fn apply_vscode_copilot(
    mem_dir: &Path,
    project_root: &Path,
    manifest: &Value,
) -> Result<ApplyResult, IdeError> {
    let mut written = Vec::new();
    let out_dir = project_root.join(".github").join("instructions");
    ensure_dir(&out_dir)?;
    for (name, cfg) in agents_iter(manifest) {
        let ctx = read_context(mem_dir, cfg.get("context_file").and_then(Value::as_str));
        let label = agent_label(&cfg, &name);
        let desc_raw = cfg
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("");
        let md = format!(
            "---\napplyTo: \"**\"\n---\n\n# {label}\n> {desc_raw}\n\n{ctx}\n",
            ctx = trim(&ctx),
        );
        let out = out_dir.join(format!("{name}.instructions.md"));
        fs::write(&out, md)?;
        written.push(path_rel(project_root, &out));
    }
    // índice global
    let main_path = project_root.join(".github").join("copilot-instructions.md");
    if let Some(parent) = main_path.parent() {
        ensure_dir(parent)?;
    }
    // Replica `[header, '', desc, '', ...items, ''].join('\n')` do .cjs.
    let mut parts: Vec<String> = vec![
        "# Agentes disponíveis (project-mem)".into(),
        String::new(),
        "Este projeto usa project-mem. Os agentes abaixo têm arquivo próprio em .github/instructions/:".into(),
        String::new(),
    ];
    for (name, cfg) in agents_iter(manifest) {
        let desc = agent_desc_or_label_or_name(&cfg, &name);
        parts.push(format!("- **{name}** — {desc}"));
    }
    parts.push(String::new());
    fs::write(&main_path, parts.join("\n"))?;
    written.push(".github/copilot-instructions.md".into());
    Ok(ApplyResult { written })
}

pub fn apply_codex(
    mem_dir: &Path,
    project_root: &Path,
    manifest: &Value,
) -> Result<ApplyResult, IdeError> {
    let mut written = Vec::new();
    let out_dir = project_root.join(".codex").join("instructions");
    ensure_dir(&out_dir)?;
    for (name, cfg) in agents_iter(manifest) {
        let ctx = read_context(mem_dir, cfg.get("context_file").and_then(Value::as_str));
        let label = agent_label(&cfg, &name);
        let desc_raw = cfg
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("");
        let md = format!(
            "# {label}\n> {desc_raw}\n\n{ctx}\n",
            ctx = trim(&ctx),
        );
        let out = out_dir.join(format!("{name}.md"));
        fs::write(&out, md)?;
        written.push(path_rel(project_root, &out));
    }
    Ok(ApplyResult { written })
}

pub fn apply(ide: &str, mem_dir: &Path, project_root: &Path) -> Result<ApplyResult, IdeError> {
    let ide = Ide::parse(ide)?;
    let manifest = read_manifest(mem_dir)?;
    match ide {
        Ide::ClaudeCode => apply_claude_code(mem_dir, project_root, &manifest),
        Ide::Cursor => apply_cursor(mem_dir, project_root, &manifest),
        Ide::VscodeCopilot => apply_vscode_copilot(mem_dir, project_root, &manifest),
        Ide::Codex => apply_codex(mem_dir, project_root, &manifest),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    fn seed(mem: &Path, proj: &Path) {
        let manifest = json!({
            "agents": {
                "manager": {
                    "label": "Manager",
                    "description": "Orquestra agentes",
                    "context_file": "ctx_manager.md"
                },
                "worker": {
                    "label": "Worker"
                }
            }
        });
        fs::write(mem.join("agents.json"), manifest.to_string()).unwrap();
        fs::write(mem.join("ctx_manager.md"), "  conteúdo do manager  \n").unwrap();
        let _ = proj; // not used for seed
    }

    #[test]
    fn supported_list_is_stable() {
        assert_eq!(SUPPORTED, ["claude-code", "cursor", "vscode-copilot", "codex"]);
    }

    #[test]
    fn unsupported_errors() {
        let r = Ide::parse("sublime");
        assert!(matches!(r, Err(IdeError::Unsupported(_))));
    }

    #[test]
    fn apply_claude_code_writes_agent_files() {
        let mem = TempDir::new().unwrap();
        let proj = TempDir::new().unwrap();
        seed(mem.path(), proj.path());
        let r = apply("claude-code", mem.path(), proj.path()).unwrap();
        assert_eq!(r.written.len(), 2);
        assert!(r.written.iter().any(|p| p == ".claude/agents/manager.md"));
        assert!(r.written.iter().any(|p| p == ".claude/agents/worker.md"));
        let md = fs::read_to_string(proj.path().join(".claude/agents/manager.md")).unwrap();
        assert!(md.contains("name: manager"));
        assert!(md.contains("description: Orquestra agentes"));
        assert!(md.contains("conteúdo do manager"));
    }

    #[test]
    fn apply_cursor_writes_mdc_and_cursorrules() {
        let mem = TempDir::new().unwrap();
        let proj = TempDir::new().unwrap();
        seed(mem.path(), proj.path());
        let r = apply("cursor", mem.path(), proj.path()).unwrap();
        assert_eq!(r.written.last().unwrap(), ".cursorrules");
        let rules = fs::read_to_string(proj.path().join(".cursorrules")).unwrap();
        assert!(rules.contains("- **manager** — Orquestra agentes"));
        let mdc = fs::read_to_string(proj.path().join(".cursor/rules/worker.mdc")).unwrap();
        assert!(mdc.contains("# Worker"));
        assert!(mdc.contains("alwaysApply: false"));
    }

    #[test]
    fn apply_vscode_copilot_writes_instructions() {
        let mem = TempDir::new().unwrap();
        let proj = TempDir::new().unwrap();
        seed(mem.path(), proj.path());
        let r = apply("vscode-copilot", mem.path(), proj.path()).unwrap();
        assert!(r
            .written
            .contains(&".github/copilot-instructions.md".to_string()));
        let main =
            fs::read_to_string(proj.path().join(".github/copilot-instructions.md")).unwrap();
        assert!(main.contains("- **manager** — Orquestra agentes"));
    }

    #[test]
    fn apply_codex_writes_instructions() {
        let mem = TempDir::new().unwrap();
        let proj = TempDir::new().unwrap();
        seed(mem.path(), proj.path());
        let r = apply("codex", mem.path(), proj.path()).unwrap();
        let md = fs::read_to_string(proj.path().join(".codex/instructions/manager.md")).unwrap();
        assert!(md.starts_with("# Manager\n> Orquestra agentes\n"));
        assert!(r.written.iter().any(|p| p == ".codex/instructions/manager.md"));
    }

    #[test]
    fn manifest_missing_errors() {
        let mem = TempDir::new().unwrap();
        let proj = TempDir::new().unwrap();
        let r = apply("claude-code", mem.path(), proj.path());
        assert!(matches!(r, Err(IdeError::ManifestMissing(_))));
    }

    #[test]
    fn missing_context_file_is_empty_string() {
        let mem = TempDir::new().unwrap();
        let proj = TempDir::new().unwrap();
        // manifest aponta pra context_file inexistente.
        let manifest = json!({
            "agents": {
                "x": {"context_file": "nope.md", "description": "d"}
            }
        });
        fs::write(mem.path().join("agents.json"), manifest.to_string()).unwrap();
        let r = apply("claude-code", mem.path(), proj.path()).unwrap();
        assert_eq!(r.written.len(), 1);
        let md = fs::read_to_string(proj.path().join(".claude/agents/x.md")).unwrap();
        // Ctx vazio: trimmed = "", so md ends with "---\n\n\n"
        assert!(md.contains("---\n\n\n"));
    }

    #[test]
    fn label_fallback_to_name_when_missing() {
        let mem = TempDir::new().unwrap();
        let proj = TempDir::new().unwrap();
        let manifest = json!({"agents":{"solo":{}}});
        fs::write(mem.path().join("agents.json"), manifest.to_string()).unwrap();
        apply("cursor", mem.path(), proj.path()).unwrap();
        let mdc = fs::read_to_string(proj.path().join(".cursor/rules/solo.mdc")).unwrap();
        assert!(mdc.contains("# solo"));
    }
}
