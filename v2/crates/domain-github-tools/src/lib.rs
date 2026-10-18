//! Domain-github-tools — qualquer repositório do GitHub como ferramenta de
//! agente.
//!
//! # Modelo
//! A ferramenta vira um **MCP server stdio** no registro do projeto
//! (`.poupar/mcps.json` → `.mcp.json`). A permissão é a lista `mcps[]` de cada
//! agente: só quem tem o nome ali recebe `mcp__<nome>` no `--allowed-tools`.
//! Colar a URL e escolher os agentes é, portanto, o ato de conceder a
//! ferramenta.
//!
//! # Fixação
//! Toda ferramenta é fixada num commit (ou tag de release, no caso de
//! binários) no momento do registro. A def guarda `source.sha`; o comando
//! gerado aponta pra esse commit, não pra `main`. Atualizar é registrar de
//! novo — nunca acontece sozinho.
//!
//! # Tipos suportados
//! - `lightpanda` — `lightpanda-io/browser`: binário da release + `lightpanda mcp`.
//! - `npm` — `package.json` com `bin` → `npx -y --package=github:o/r#sha <bin>`.
//! - `python` — `pyproject.toml` com `[project.scripts]` → `uvx --from git+…@sha <script>`.
//! - `cargo` — `Cargo.toml` → `cargo install --git … --rev sha` e roda o binário.
//! - `go` — `go.mod` → `go run <módulo>@sha`.
//! - `npm-published` — monorepo com o MCP em `packages/<x>/` →
//!   `npx -y --package=<nome>@<versão> <bin>` (fixado na versão publicada, não
//!   no commit: o npx não instala subdiretório de repo git).
//! - `custom` — comando informado por quem registra (repo sem manifesto reconhecível).
//!
//! # Fora de escopo
//! `git fetch`, download e `cargo install` — I/O da camada de API
//! (`api-http::github_tools_routes`).

use std::path::Path;

use serde_json::{json, Value};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("URL do GitHub inválida: {0} (use https://github.com/<dono>/<repo>)")]
    BadUrl(String),
    #[error("ref inválida: {0}")]
    BadRef(String),
    #[error("não reconheci o tipo do projeto em {0} — informe 'command' e 'args'")]
    UnknownKind(String),
    #[error("{0}")]
    Manifest(String),
    #[error("plataforma sem binário do Lightpanda: {0}")]
    UnsupportedPlatform(String),
}

// ── URL ─────────────────────────────────────────────────────────────────

/// `owner/repo` + ref opcional, já validados.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRef {
    pub owner: String,
    pub repo: String,
    pub git_ref: Option<String>,
}

impl RepoRef {
    pub fn slug(&self) -> String {
        format!("{}/{}", self.owner, self.repo)
    }

    pub fn https_url(&self) -> String {
        format!("https://github.com/{}/{}.git", self.owner, self.repo)
    }

    pub fn is_lightpanda(&self) -> bool {
        self.owner.eq_ignore_ascii_case("lightpanda-io")
            && self.repo.eq_ignore_ascii_case("browser")
    }
}

fn is_valid_segment(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 100
        && s != "."
        && s != ".."
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Ref git aceitável: branch, tag ou SHA. Nada que vire flag (`-x`) nem
/// navegação (`..`).
pub fn is_valid_ref(r: &str) -> bool {
    !r.is_empty()
        && r.len() <= 200
        && !r.starts_with('-')
        && !r.contains("..")
        && r.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
}

/// Aceita `https://github.com/o/r`, `github.com/o/r`, `o/r`, com `.git`,
/// barra final ou `/tree/<ref>`. `explicit_ref` vence a ref da URL.
pub fn parse_github_url(input: &str, explicit_ref: Option<&str>) -> Result<RepoRef, Error> {
    let bad = || Error::BadUrl(input.to_string());
    let mut s = input.trim();
    for prefix in ["https://", "http://"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest;
        }
    }
    if let Some(rest) = s.strip_prefix("www.") {
        s = rest;
    }
    let had_host = s.starts_with("github.com/");
    if let Some(rest) = s.strip_prefix("github.com/") {
        s = rest;
    } else if input.contains("://") {
        return Err(bad());
    }
    let s = s.trim_end_matches('/');

    let mut parts = s.splitn(3, '/');
    let owner = parts.next().unwrap_or("");
    let repo = parts.next().unwrap_or("").trim_end_matches(".git");
    let rest = parts.next();
    if !is_valid_segment(owner) || !is_valid_segment(repo) {
        return Err(bad());
    }

    let url_ref = match rest {
        None => None,
        Some(r) => match r.strip_prefix("tree/") {
            Some(t) if had_host => Some(t.to_string()),
            _ => return Err(bad()),
        },
    };

    let git_ref = explicit_ref
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .map(String::from)
        .or(url_ref);
    if let Some(r) = &git_ref {
        if !is_valid_ref(r) {
            return Err(Error::BadRef(r.clone()));
        }
    }

    Ok(RepoRef {
        owner: owner.to_string(),
        repo: repo.to_string(),
        git_ref,
    })
}

/// Nome do MCP no registro (`[a-z0-9_-]`, até 64). `lightpanda-io/browser`
/// vira `lightpanda` — "browser" sozinho diria pouco no `mcp__<nome>`.
pub fn default_tool_name(r: &RepoRef) -> String {
    let base = if r.is_lightpanda() { "lightpanda" } else { &r.repo };
    let mut out: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    out.truncate(64);
    out.trim_matches('-').to_string()
}

// ── Detecção ────────────────────────────────────────────────────────────

/// Manifestos lidos da raiz do repo (só os que existirem).
#[derive(Debug, Default, Clone)]
pub struct ProjectFiles {
    pub package_json: Option<String>,
    pub pyproject_toml: Option<String>,
    pub cargo_toml: Option<String>,
    pub go_mod: Option<String>,
    /// `packages/<dir>/package.json` de monorepos, em ordem de nome.
    pub workspace_packages: Vec<(String, String)>,
}

impl ProjectFiles {
    pub fn read_from(dir: &Path) -> Self {
        let read = |n: &str| std::fs::read_to_string(dir.join(n)).ok();
        Self {
            package_json: read("package.json"),
            pyproject_toml: read("pyproject.toml"),
            cargo_toml: read("Cargo.toml"),
            go_mod: read("go.mod"),
            workspace_packages: read_workspace_packages(&dir.join("packages")),
        }
    }
}

fn read_workspace_packages(dir: &Path) -> Vec<(String, String)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(String, String)> = entries
        .flatten()
        .filter_map(|e| {
            let raw = std::fs::read_to_string(e.path().join("package.json")).ok()?;
            Some((e.file_name().to_string_lossy().into_owned(), raw))
        })
        .collect();
    out.sort();
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolKind {
    Lightpanda,
    Npm { bin: String },
    Python { script: String },
    Cargo { bin: String },
    Go { module: String },
    NpmPublished { package: String, version: String, bin: String },
}

impl ToolKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ToolKind::Lightpanda => "lightpanda",
            ToolKind::Npm { .. } => "npm",
            ToolKind::Python { .. } => "python",
            ToolKind::Cargo { .. } => "cargo",
            ToolKind::Go { .. } => "go",
            ToolKind::NpmPublished { .. } => "npm-published",
        }
    }
}

/// Ordem: npm → python → cargo → go → pacote de monorepo. Um repo com mais de um manifesto quase
/// sempre publica o MCP pelo primeiro (o Cargo.toml costuma ser de uma
/// extensão nativa).
pub fn detect_kind(r: &RepoRef, files: &ProjectFiles) -> Result<ToolKind, Error> {
    if r.is_lightpanda() {
        return Ok(ToolKind::Lightpanda);
    }
    if let Some(raw) = &files.package_json {
        if let Some(bin) = npm_bin(raw, &r.repo)? {
            return Ok(ToolKind::Npm { bin });
        }
    }
    if let Some(raw) = &files.pyproject_toml {
        if let Some(script) = python_script(raw)? {
            return Ok(ToolKind::Python { script });
        }
    }
    if let Some(raw) = &files.cargo_toml {
        if let Some(bin) = cargo_bin(raw)? {
            return Ok(ToolKind::Cargo { bin });
        }
    }
    if let Some(module) = files.go_mod.as_deref().and_then(go_module) {
        return Ok(ToolKind::Go { module });
    }
    if let Some(kind) = workspace_package(&files.workspace_packages) {
        return Ok(kind);
    }
    Err(Error::UnknownKind(r.slug()))
}

/// `bin` do package.json: string (nome = `name` sem escopo) ou objeto
/// (prefere a entrada com o nome do repo, senão a primeira).
fn npm_bin(raw: &str, repo: &str) -> Result<Option<String>, Error> {
    let v: Value = serde_json::from_str(raw)
        .map_err(|e| Error::Manifest(format!("package.json inválido: {e}")))?;
    let name = v.get("name").and_then(Value::as_str).unwrap_or(repo);
    let unscoped = name.rsplit('/').next().unwrap_or(name);
    Ok(match v.get("bin") {
        Some(Value::String(_)) => Some(unscoped.to_string()),
        Some(Value::Object(m)) if !m.is_empty() => Some(
            m.keys()
                .find(|k| k.as_str() == repo || k.as_str() == unscoped)
                .unwrap_or_else(|| m.keys().next().unwrap())
                .clone(),
        ),
        _ => None,
    })
}

fn python_script(raw: &str) -> Result<Option<String>, Error> {
    let v: toml::Value =
        toml::from_str(raw).map_err(|e| Error::Manifest(format!("pyproject.toml inválido: {e}")))?;
    Ok(v.get("project")
        .and_then(|p| p.get("scripts"))
        .and_then(toml::Value::as_table)
        .and_then(|t| t.keys().next().cloned()))
}

/// Primeiro `[[bin]]`; sem isso, `package.name` (bin implícito de
/// `src/main.rs`). Workspace sem `[package]` não é instalável direto.
fn cargo_bin(raw: &str) -> Result<Option<String>, Error> {
    let v: toml::Value =
        toml::from_str(raw).map_err(|e| Error::Manifest(format!("Cargo.toml inválido: {e}")))?;
    if let Some(name) = v
        .get("bin")
        .and_then(toml::Value::as_array)
        .and_then(|a| a.first())
        .and_then(|b| b.get("name"))
        .and_then(toml::Value::as_str)
    {
        return Ok(Some(name.to_string()));
    }
    Ok(v.get("package")
        .and_then(|p| p.get("name"))
        .and_then(toml::Value::as_str)
        .map(String::from))
}

/// Pacote de monorepo com `bin`, `name` e `version` — prefere um com "mcp"
/// no nome; pacote `private` nunca foi publicado, então não serve.
fn workspace_package(pkgs: &[(String, String)]) -> Option<ToolKind> {
    let candidates: Vec<(String, String, String)> = pkgs
        .iter()
        .filter_map(|(dir, raw)| {
            let v: Value = serde_json::from_str(raw).ok()?;
            if v.get("private").and_then(Value::as_bool) == Some(true) {
                return None;
            }
            let name = v.get("name")?.as_str()?.to_string();
            let version = v.get("version")?.as_str()?.to_string();
            let bin = npm_bin(raw, dir).ok().flatten()?;
            Some((name, version, bin))
        })
        .collect();
    let pick = candidates
        .iter()
        .find(|(n, _, b)| n.contains("mcp") || b.contains("mcp"))
        .or_else(|| candidates.first())?;
    Some(ToolKind::NpmPublished {
        package: pick.0.clone(),
        version: pick.1.clone(),
        bin: pick.2.clone(),
    })
}

/// Linha `module <caminho>` do go.mod.
fn go_module(raw: &str) -> Option<String> {
    raw.lines()
        .find_map(|l| l.trim().strip_prefix("module "))
        .map(|m| m.trim().trim_matches('"').to_string())
        .filter(|m| !m.is_empty() && !m.starts_with('-') && !m.contains(char::is_whitespace))
}

// ── Lightpanda ──────────────────────────────────────────────────────────

/// Asset da release do Lightpanda pra `std::env::consts::{OS, ARCH}`.
pub fn lightpanda_asset(os: &str, arch: &str) -> Result<&'static str, Error> {
    match (os, arch) {
        ("macos", "aarch64") => Ok("lightpanda-aarch64-macos"),
        ("macos", "x86_64") => Ok("lightpanda-x86_64-macos"),
        ("linux", "aarch64") => Ok("lightpanda-aarch64-linux"),
        ("linux", "x86_64") => Ok("lightpanda-x86_64-linux"),
        _ => Err(Error::UnsupportedPlatform(format!("{os}/{arch}"))),
    }
}

/// Extrai `tag_name` do JSON de `/releases/latest`.
pub fn release_tag(json_body: &str) -> Option<String> {
    serde_json::from_str::<Value>(json_body)
        .ok()?
        .get("tag_name")?
        .as_str()
        .filter(|t| is_valid_ref(t))
        .map(String::from)
}

// ── Def do MCP ──────────────────────────────────────────────────────────

/// `command` + `args` do MCP stdio. `installed_bin` é o binário já
/// instalado em disco (Lightpanda e cargo); npm/python resolvem sozinhos.
pub fn command_for(
    r: &RepoRef,
    sha: &str,
    kind: &ToolKind,
    installed_bin: Option<&Path>,
) -> (String, Vec<String>) {
    let bin_path = || {
        installed_bin
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    match kind {
        ToolKind::Lightpanda => (bin_path(), vec!["mcp".into()]),
        ToolKind::Cargo { .. } => (bin_path(), vec![]),
        ToolKind::Npm { bin } => (
            "npx".into(),
            vec![
                "-y".into(),
                format!("--package=github:{}#{sha}", r.slug()),
                bin.clone(),
            ],
        ),
        ToolKind::Python { script } => (
            "uvx".into(),
            vec![
                "--from".into(),
                format!("git+https://github.com/{}@{sha}", r.slug()),
                script.clone(),
            ],
        ),
        ToolKind::Go { module } => ("go".into(), vec!["run".into(), format!("{module}@{sha}")]),
        ToolKind::NpmPublished { package, version, bin } => (
            "npx".into(),
            vec!["-y".into(), format!("--package={package}@{version}"), bin.clone()],
        ),
    }
}

/// Def no formato do registro (`mcp_routes`), com `source` pra rastrear de
/// onde veio e em que commit.
pub fn mcp_def(
    name: &str,
    r: &RepoRef,
    sha: &str,
    kind: &str,
    command: &str,
    args: &[String],
) -> Value {
    json!({
        "name": name,
        "transport": "stdio",
        "description": format!("GitHub: {} @ {}", r.slug(), short_sha(sha)),
        "command": command,
        "args": args,
        "source": {
            "type": "github",
            "url": format!("https://github.com/{}", r.slug()),
            "ref": r.git_ref,
            "sha": sha,
            "kind": kind,
        },
    })
}

fn short_sha(sha: &str) -> &str {
    if sha.len() == 40 && sha.chars().all(|c| c.is_ascii_hexdigit()) {
        &sha[..12]
    } else {
        sha
    }
}

/// Diretório de instalação de uma versão: `<owner>__<repo>/<sha>`.
pub fn install_subdir(r: &RepoRef, sha: &str) -> String {
    format!("{}__{}/{}", r.owner, r.repo, sha.replace('/', "_"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(s: &str) -> RepoRef {
        parse_github_url(s, None).unwrap()
    }

    #[test]
    fn parse_accepts_common_forms() {
        for s in [
            "https://github.com/acme/tool",
            "https://github.com/acme/tool/",
            "https://github.com/acme/tool.git",
            "http://www.github.com/acme/tool",
            "github.com/acme/tool",
            "acme/tool",
        ] {
            let r = repo(s);
            assert_eq!((r.owner.as_str(), r.repo.as_str()), ("acme", "tool"), "{s}");
            assert_eq!(r.git_ref, None, "{s}");
        }
    }

    #[test]
    fn parse_takes_ref_from_tree_and_explicit_wins() {
        assert_eq!(
            repo("https://github.com/acme/tool/tree/v1.2.0").git_ref.as_deref(),
            Some("v1.2.0")
        );
        let r = parse_github_url("https://github.com/acme/tool/tree/main", Some("v2")).unwrap();
        assert_eq!(r.git_ref.as_deref(), Some("v2"));
    }

    #[test]
    fn parse_rejects_other_hosts_and_garbage() {
        for s in [
            "https://gitlab.com/acme/tool",
            "https://github.com/acme",
            "https://github.com/acme/tool/issues/3",
            "acme",
            "",
            "../etc/passwd",
            "acme/to ol",
        ] {
            assert!(parse_github_url(s, None).is_err(), "{s}");
        }
    }

    #[test]
    fn parse_rejects_refs_that_look_like_flags_or_paths() {
        for r in ["--upload-pack=x", "a..b", "a b", "$(x)"] {
            assert_eq!(
                parse_github_url("acme/tool", Some(r)),
                Err(Error::BadRef(r.into()))
            );
        }
    }

    #[test]
    fn default_name_sanitizes_and_special_cases_lightpanda() {
        assert_eq!(default_tool_name(&repo("lightpanda-io/browser")), "lightpanda");
        assert_eq!(default_tool_name(&repo("acme/My.Tool_v2")), "my-tool-v2");
    }

    #[test]
    fn detect_lightpanda_without_manifests() {
        let r = repo("https://github.com/lightpanda-io/browser");
        assert_eq!(detect_kind(&r, &ProjectFiles::default()), Ok(ToolKind::Lightpanda));
    }

    #[test]
    fn detect_npm_bin_string_and_object() {
        let r = repo("acme/tool");
        let f = ProjectFiles {
            package_json: Some(r#"{"name":"@acme/tool-mcp","bin":"dist/index.js"}"#.into()),
            ..Default::default()
        };
        assert_eq!(detect_kind(&r, &f), Ok(ToolKind::Npm { bin: "tool-mcp".into() }));

        let f = ProjectFiles {
            package_json: Some(r#"{"name":"x","bin":{"other":"a.js","tool":"b.js"}}"#.into()),
            ..Default::default()
        };
        assert_eq!(detect_kind(&r, &f), Ok(ToolKind::Npm { bin: "tool".into() }));
    }

    #[test]
    fn detect_falls_through_npm_without_bin_to_python() {
        let r = repo("acme/tool");
        let f = ProjectFiles {
            package_json: Some(r#"{"name":"docs-site"}"#.into()),
            pyproject_toml: Some("[project]\nname='t'\n[project.scripts]\ntool-mcp = 'tool:main'\n".into()),
            ..Default::default()
        };
        assert_eq!(detect_kind(&r, &f), Ok(ToolKind::Python { script: "tool-mcp".into() }));
    }

    #[test]
    fn detect_cargo_prefers_explicit_bin() {
        let r = repo("acme/tool");
        let f = ProjectFiles {
            cargo_toml: Some("[package]\nname='tool'\n[[bin]]\nname='tool-mcp'\npath='src/m.rs'\n".into()),
            ..Default::default()
        };
        assert_eq!(detect_kind(&r, &f), Ok(ToolKind::Cargo { bin: "tool-mcp".into() }));
        let f = ProjectFiles {
            cargo_toml: Some("[package]\nname='tool'\n".into()),
            ..Default::default()
        };
        assert_eq!(detect_kind(&r, &f), Ok(ToolKind::Cargo { bin: "tool".into() }));
    }

    #[test]
    fn detect_go_module() {
        let r = repo("acme/tool");
        let f = ProjectFiles {
            go_mod: Some("// c\nmodule github.com/acme/tool\n\ngo 1.22\n".into()),
            ..Default::default()
        };
        let k = detect_kind(&r, &f).unwrap();
        assert_eq!(k, ToolKind::Go { module: "github.com/acme/tool".into() });
        let (cmd, args) = command_for(&r, "abc", &k, None);
        assert_eq!((cmd.as_str(), args), ("go", vec!["run".into(), "github.com/acme/tool@abc".to_string()]));
    }

    #[test]
    fn detect_monorepo_package_prefers_mcp_and_skips_private() {
        let r = repo("acme/tool");
        let f = ProjectFiles {
            package_json: Some(r#"{"name":"root","private":true}"#.into()),
            workspace_packages: vec![
                ("cli".into(), r#"{"name":"@acme/cli","version":"1.0.0","bin":"x.js"}"#.into()),
                ("internal".into(), r#"{"name":"@acme/mcp-dev","version":"0.0.1","private":true,"bin":"d.js"}"#.into()),
                ("mcp".into(), r#"{"name":"@acme/tool-mcp","version":"2.3.4","bin":{"tool-mcp":"dist/i.js"}}"#.into()),
            ],
            ..Default::default()
        };
        let k = detect_kind(&r, &f).unwrap();
        assert_eq!(
            k,
            ToolKind::NpmPublished { package: "@acme/tool-mcp".into(), version: "2.3.4".into(), bin: "tool-mcp".into() }
        );
        let (cmd, args) = command_for(&r, "sha", &k, None);
        assert_eq!((cmd.as_str(), args[1].as_str()), ("npx", "--package=@acme/tool-mcp@2.3.4"));
    }

    #[test]
    fn detect_unknown_asks_for_command() {
        let r = repo("acme/tool");
        assert_eq!(
            detect_kind(&r, &ProjectFiles::default()),
            Err(Error::UnknownKind("acme/tool".into()))
        );
    }

    #[test]
    fn command_pins_to_sha() {
        let r = repo("acme/tool");
        let sha = "0123456789abcdef0123456789abcdef01234567";
        let (cmd, args) = command_for(&r, sha, &ToolKind::Npm { bin: "tool".into() }, None);
        assert_eq!(cmd, "npx");
        assert_eq!(args, vec!["-y", &format!("--package=github:acme/tool#{sha}"), "tool"]);

        let (cmd, args) = command_for(&r, sha, &ToolKind::Python { script: "t".into() }, None);
        assert_eq!(cmd, "uvx");
        assert_eq!(args[1], format!("git+https://github.com/acme/tool@{sha}"));
    }

    #[test]
    fn lightpanda_runs_installed_binary_in_mcp_mode() {
        let r = repo("lightpanda-io/browser");
        let (cmd, args) = command_for(&r, "1.0.1", &ToolKind::Lightpanda, Some(Path::new("/t/lightpanda")));
        assert_eq!((cmd.as_str(), args), ("/t/lightpanda", vec!["mcp".to_string()]));
    }

    #[test]
    fn lightpanda_assets_per_platform() {
        assert_eq!(lightpanda_asset("macos", "aarch64"), Ok("lightpanda-aarch64-macos"));
        assert_eq!(lightpanda_asset("linux", "x86_64"), Ok("lightpanda-x86_64-linux"));
        assert!(lightpanda_asset("windows", "x86_64").is_err());
    }

    #[test]
    fn release_tag_reads_tag_name() {
        assert_eq!(release_tag(r#"{"tag_name":"1.0.1"}"#).as_deref(), Some("1.0.1"));
        assert_eq!(release_tag(r#"{"message":"Not Found"}"#), None);
    }

    #[test]
    fn mcp_def_records_source() {
        let r = parse_github_url("acme/tool", Some("v1")).unwrap();
        let sha = "0123456789abcdef0123456789abcdef01234567";
        let d = mcp_def("tool", &r, sha, "npm", "npx", &["-y".into()]);
        assert_eq!(d["transport"], "stdio");
        assert_eq!(d["source"]["sha"], sha);
        assert_eq!(d["source"]["ref"], "v1");
        assert_eq!(d["description"], "GitHub: acme/tool @ 0123456789ab");
    }
}
