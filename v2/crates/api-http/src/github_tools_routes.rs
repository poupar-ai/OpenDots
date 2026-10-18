//! Repositório do GitHub → ferramenta (MCP stdio) de agentes.
//!
//! - `POST /api/tools/github` → body
//!   `{ url, ref?, name?, agents?: string[] | "*", command?, args?, env?, dry_run? }`
//!
//! Busca o repo fixado num commit, detecta o tipo (`domain-github-tools`),
//! instala o que precisar fora do repositório do projeto e registra no
//! catálogo de MCPs (`mcp_routes::register_def`). `agents` anexa a
//! ferramenta a esses agentes (`mcps[]`) — é isso que libera
//! `mcp__<nome>` no `--allowed-tools` deles. `"*"` = todos.
//!
//! Repo sem manifesto reconhecido entra com `command`/`args` explícitos;
//! `{repo}` neles vira o caminho do checkout. Não há passo de build
//! arbitrário: esta rota nunca roda shell vindo do request — o comando só
//! executa quando o Claude Code sobe o MCP, como qualquer MCP do catálogo.
//!
//! `dry_run: true` só busca e detecta: devolve a def sem instalar, sem
//! registrar e sem tocar agentes — é o "Analisar" da UI.
//!
//! Instalações ficam em `$POUPAR_TOOLS_DIR` ou
//! `${XDG_DATA_HOME:-~/.local/share}/open-poupar/tools/<dono>__<repo>/<sha>`:
//! binário de terceiro não vai pro `.poupar/` (que é versionado no git).

use std::path::{Path, PathBuf};
use std::process::Command;

use axum::body::Bytes;
use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::post;
use axum::Router;
use domain_github_tools as gt;
use serde_json::{json, Value};

use crate::mcp_routes::{is_registered, register_def};
use crate::tenant::TenantContext;
use crate::{json_error, json_response, AppState};

pub fn github_tools_router() -> Router<AppState> {
    Router::new().route("/api/tools/github", post(install_handler))
}

fn tools_root() -> PathBuf {
    if let Some(d) = std::env::var_os("POUPAR_TOOLS_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(d);
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
        });
    data.join("open-poupar").join("tools")
}

/// Comando sem prompt de credencial: repo privado sem acesso falha na
/// hora em vez de pendurar o request esperando senha.
fn run(cmd: &mut Command, what: &str) -> Result<String, String> {
    let out = cmd
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| format!("{what}: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let tail: String = err.lines().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
        return Err(format!("{what} falhou: {tail}"));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn has_cmd(name: &str) -> bool {
    Command::new("sh")
        .args(["-c", "command -v \"$1\" >/dev/null 2>&1", "sh", name])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Checkout raso de `r` (ref ou HEAD) em `dest`. Devolve o SHA.
/// `fetch <url> <ref>` em vez de `clone --branch` porque aceita SHA também.
fn fetch_repo(r: &gt::RepoRef, dest: &Path) -> Result<String, String> {
    if dest.join(".git").is_dir() {
        return run(Command::new("git").arg("-C").arg(dest).args(["rev-parse", "HEAD"]), "git rev-parse");
    }
    let tmp = dest.with_extension("partial");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| format!("criar {}: {e}", tmp.display()))?;
    let git = |args: &[&str], what: &str| {
        run(Command::new("git").arg("-C").arg(&tmp).args(args), what)
    };
    git(&["init", "-q"], "git init")?;
    let refspec = r.git_ref.as_deref().unwrap_or("HEAD");
    git(&["fetch", "-q", "--depth", "1", &r.https_url(), refspec], "git fetch")?;
    git(&["-c", "advice.detachedHead=false", "checkout", "-q", "FETCH_HEAD"], "git checkout")?;
    let sha = git(&["rev-parse", "HEAD"], "git rev-parse")?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let _ = std::fs::remove_dir_all(dest);
    std::fs::rename(&tmp, dest).map_err(|e| format!("mover checkout: {e}"))?;
    Ok(sha)
}

/// Baixa o binário do Lightpanda da release (tag = ref ou a última).
fn install_lightpanda(r: &gt::RepoRef, root: &Path, dry_run: bool) -> Result<(String, PathBuf), String> {
    let asset = gt::lightpanda_asset(std::env::consts::OS, std::env::consts::ARCH)
        .map_err(|e| e.to_string())?;
    let tag = match &r.git_ref {
        Some(t) => t.clone(),
        None => {
            let body = run(
                Command::new("curl").args([
                    "-fsSL",
                    "https://api.github.com/repos/lightpanda-io/browser/releases/latest",
                ]),
                "consultar última release do Lightpanda",
            )?;
            gt::release_tag(&body).ok_or("release do Lightpanda sem tag_name")?
        }
    };
    let dir = root.join(gt::install_subdir(r, &tag));
    let bin = dir.join("lightpanda");
    if dry_run || bin.is_file() {
        return Ok((tag, bin));
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let url = format!("https://github.com/lightpanda-io/browser/releases/download/{tag}/{asset}");
    let part = dir.join("lightpanda.partial");
    run(
        Command::new("curl").args(["-fsSL", "--retry", "3", "-o"]).arg(&part).arg(&url),
        &format!("baixar {url}"),
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    std::fs::rename(&part, &bin).map_err(|e| e.to_string())?;
    Ok((tag, bin))
}

/// Substitui `{repo}` pelo caminho do checkout.
fn expand_repo(s: &str, checkout: &Path) -> String {
    s.replace("{repo}", &checkout.to_string_lossy())
}

struct Request {
    repo: gt::RepoRef,
    name: String,
    command: Option<String>,
    args: Vec<String>,
    env: Option<Value>,
    dry_run: bool,
}

struct Installed {
    def: Value,
    kind: String,
    warnings: Vec<String>,
}

fn parse_request(v: &Value) -> Result<(Request, Value), String> {
    let url = v.get("url").and_then(Value::as_str).unwrap_or("");
    let repo = gt::parse_github_url(url, v.get("ref").and_then(Value::as_str))
        .map_err(|e| e.to_string())?;
    let name = v
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(String::from)
        .unwrap_or_else(|| gt::default_tool_name(&repo));
    if name.is_empty()
        || name.len() > 64
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("nome inválido (use [a-z0-9_-], até 64)".into());
    }
    let command = v
        .get("command")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(String::from);
    let args = v
        .get("args")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect())
        .unwrap_or_default();
    let req = Request {
        repo,
        name,
        command,
        args,
        env: v.get("env").filter(|e| e.is_object()).cloned(),
        dry_run: v.get("dry_run").and_then(Value::as_bool).unwrap_or(false),
    };
    Ok((req, v.get("agents").cloned().unwrap_or(Value::Null)))
}

fn install(req: &Request) -> Result<Installed, String> {
    let root = tools_root();
    let mut warnings = Vec::new();

    if req.repo.is_lightpanda() && req.command.is_none() {
        let (tag, bin) = install_lightpanda(&req.repo, &root, req.dry_run)?;
        let (cmd, args) = gt::command_for(&req.repo, &tag, &gt::ToolKind::Lightpanda, Some(&bin));
        let def = gt::mcp_def(&req.name, &req.repo, &tag, "lightpanda", &cmd, &args);
        return Ok(Installed { def, kind: "lightpanda".into(), warnings });
    }

    // Checkout primeiro num diretório por ref; depois de saber o SHA,
    // fica em `<dono>__<repo>/<sha>/repo`.
    let staging = root.join(format!(
        "{}__{}/.fetch-{}",
        req.repo.owner,
        req.repo.repo,
        req.repo.git_ref.as_deref().unwrap_or("HEAD").replace('/', "_")
    ));
    let _ = std::fs::remove_dir_all(&staging);
    let sha = fetch_repo(&req.repo, &staging)?;
    let base = root.join(gt::install_subdir(&req.repo, &sha));
    let checkout = base.join("repo");
    if checkout.join(".git").is_dir() {
        let _ = std::fs::remove_dir_all(&staging);
    } else {
        std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;
        std::fs::rename(&staging, &checkout).map_err(|e| format!("mover checkout: {e}"))?;
    }

    let (kind, cmd, args) = if let Some(cmd) = &req.command {
        let args: Vec<String> = req.args.iter().map(|a| expand_repo(a, &checkout)).collect();
        ("custom".to_string(), expand_repo(cmd, &checkout), args)
    } else {
        let files = gt::ProjectFiles::read_from(&checkout);
        let kind = gt::detect_kind(&req.repo, &files).map_err(|e| e.to_string())?;
        let installed_bin = match &kind {
            gt::ToolKind::Cargo { bin } => {
                let bin_path = base.join("bin").join(bin);
                if !req.dry_run && !bin_path.is_file() {
                    run(
                        Command::new("cargo")
                            .args(["install", "--quiet", "--git"])
                            .arg(req.repo.https_url())
                            .args(["--rev", &sha, "--bin", bin, "--root"])
                            .arg(&base),
                        "cargo install",
                    )?;
                }
                Some(bin_path)
            }
            _ => None,
        };
        if let gt::ToolKind::NpmPublished { package, version, .. } = &kind {
            warnings.push(format!(
                "monorepo: roda {package}@{version} publicado no npm (fixado na versão, não no commit)"
            ));
        }
        let (cmd, args) = gt::command_for(&req.repo, &sha, &kind, installed_bin.as_deref());
        (kind.as_str().to_string(), cmd, args)
    };

    if !cmd.contains('/') && !has_cmd(&cmd) {
        warnings.push(format!(
            "'{cmd}' não está no PATH deste servidor — instale antes de usar a ferramenta"
        ));
    }

    let mut def = gt::mcp_def(&req.name, &req.repo, &sha, &kind, &cmd, &args);
    if let (Some(env), Some(o)) = (&req.env, def.as_object_mut()) {
        o.insert("env".into(), env.clone());
    }
    Ok(Installed { def, kind, warnings })
}

/// Anexa `name` ao `mcps[]` dos agentes pedidos (`"*"` = todos).
/// Devolve (anexados, inexistentes).
fn attach_to_agents(mem_dir: &Path, name: &str, agents: &Value) -> Result<(Vec<String>, Vec<String>), String> {
    let wanted: Option<Vec<String>> = match agents {
        Value::String(s) if s == "*" => None,
        Value::Array(a) => Some(a.iter().filter_map(Value::as_str).map(String::from).collect()),
        Value::Null => return Ok((vec![], vec![])),
        _ => return Err("agents: string[] ou \"*\"".into()),
    };
    if wanted.as_ref().is_some_and(Vec::is_empty) {
        return Ok((vec![], vec![]));
    }
    let mut manifest = domain_agents::load_manifest(mem_dir).ok_or("time de agentes não configurado")?;
    let Some(all) = manifest.get_mut("agents").and_then(Value::as_object_mut) else {
        return Err("agents.json sem 'agents'".into());
    };
    let targets: Vec<String> = wanted.clone().unwrap_or_else(|| all.keys().cloned().collect());
    let mut attached = Vec::new();
    let mut missing = Vec::new();
    for agent in targets {
        let Some(obj) = all.get_mut(&agent).and_then(Value::as_object_mut) else {
            missing.push(agent);
            continue;
        };
        let mcps = obj.entry("mcps").or_insert_with(|| json!([]));
        if !mcps.is_array() {
            *mcps = json!([]);
        }
        let arr = mcps.as_array_mut().unwrap();
        if !arr.iter().any(|m| m.as_str() == Some(name)) {
            arr.push(json!(name));
        }
        attached.push(agent);
    }
    domain_agents::save_manifest(mem_dir, &manifest).map_err(|e| e.to_string())?;
    Ok((attached, missing))
}

async fn install_handler(
    State(st): State<AppState>,
    Extension(tc): Extension<TenantContext>,
    body: Bytes,
) -> Response {
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid json"),
    };
    let (req, agents) = match parse_request(&v) {
        Ok(r) => r,
        Err(e) => return json_error(StatusCode::BAD_REQUEST, &e),
    };
    let dry_run = req.dry_run;
    let name = req.name.clone();

    // git/curl/cargo bloqueiam (cargo install pode levar minutos).
    let installed = match tokio::task::spawn_blocking(move || install(&req)).await {
        Ok(Ok(i)) => i,
        Ok(Err(e)) => return json_error(StatusCode::UNPROCESSABLE_ENTITY, &e),
        Err(e) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };

    if dry_run {
        return json_response(
            StatusCode::OK,
            json!({
                "ok": true,
                "dry_run": true,
                "kind": installed.kind,
                "mcp": installed.def,
                "exists": is_registered(&tc.mem_dir, &name),
                "warnings": installed.warnings,
            }),
        );
    }

    if let Err((code, msg)) = register_def(&tc.mem_dir, &st.project_root, &st.vault, &installed.def) {
        return json_error(code, &msg);
    }
    let (attached, missing) = match attach_to_agents(&tc.mem_dir, &name, &agents) {
        Ok(r) => r,
        Err(e) => return json_error(StatusCode::BAD_REQUEST, &format!("registrado, mas: {e}")),
    };

    json_response(
        StatusCode::OK,
        json!({
            "ok": true,
            "kind": installed.kind,
            "mcp": installed.def,
            "attached": attached,
            "missing_agents": missing,
            "warnings": installed.warnings,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_request_defaults_name_from_repo() {
        let (r, agents) = parse_request(&json!({ "url": "https://github.com/acme/Cool.Tool" })).unwrap();
        assert_eq!(r.name, "cool-tool");
        assert!(!r.dry_run);
        assert!(agents.is_null());
    }

    #[test]
    fn parse_request_rejects_bad_url_and_name() {
        assert!(parse_request(&json!({ "url": "https://gitlab.com/a/b" })).is_err());
        assert!(parse_request(&json!({ "url": "a/b", "name": "x y" })).is_err());
    }

    #[test]
    fn expand_repo_placeholder() {
        assert_eq!(expand_repo("{repo}/dist/i.js", Path::new("/t/r")), "/t/r/dist/i.js");
    }

    fn manifest_with(dir: &Path, agents: Value) {
        domain_agents::save_manifest(dir, &json!({ "agents": agents })).unwrap();
    }

    #[test]
    fn attach_adds_once_and_reports_missing() {
        let dir = tempfile::tempdir().unwrap();
        manifest_with(dir.path(), json!({ "dev": { "mcps": ["x"] }, "qa": {} }));
        let (att, miss) = attach_to_agents(dir.path(), "tool", &json!(["dev", "ghost"])).unwrap();
        assert_eq!((att, miss), (vec!["dev".to_string()], vec!["ghost".to_string()]));
        attach_to_agents(dir.path(), "tool", &json!(["dev"])).unwrap();
        let m = domain_agents::load_manifest(dir.path()).unwrap();
        assert_eq!(m["agents"]["dev"]["mcps"], json!(["x", "tool"]));
        assert!(m["agents"]["qa"].get("mcps").is_none());
    }

    #[test]
    fn attach_star_means_every_agent() {
        let dir = tempfile::tempdir().unwrap();
        manifest_with(dir.path(), json!({ "dev": {}, "qa": { "mcps": "lixo" } }));
        let (att, _) = attach_to_agents(dir.path(), "tool", &json!("*")).unwrap();
        assert_eq!(att.len(), 2);
        let m = domain_agents::load_manifest(dir.path()).unwrap();
        assert_eq!(m["agents"]["qa"]["mcps"], json!(["tool"]));
    }

    #[test]
    fn attach_without_agents_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(attach_to_agents(dir.path(), "t", &Value::Null).unwrap(), (vec![], vec![]));
    }
}
