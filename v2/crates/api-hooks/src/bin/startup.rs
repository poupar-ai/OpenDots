//! `pmem-hook-startup` — thin entrypoint do `SessionStart`.
//!
//! Paridade com `plugin/scripts/startup.cjs`: coleta inputs do env/FS,
//! chama [`api_hooks::startup::build_banner`], imprime uma linha JSON.
//! Side effect opcional: autostart da UI quando o cwd está num repo git.
//!
//! **Decisão deliberada**: não portamos o autostart da UI neste binário
//! *ainda*. O `ui_server` (F4-05 `api-http`) só começa a existir depois.
//! Enquanto isso, o `.cjs` continua responsável pelo autostart; o Rust
//! **não** tenta spawnar nada — apenas emite o banner. Isso mantém o
//! cutover seguro: invocar o binário Rust não deixa a UI sem subir, pois
//! o `.cjs` ainda pode ser executado em paralelo até `api-http` entrar
//! no runtime.

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use api_hooks::startup::{
    build_banner, check_mcp_registered, count_fallback, load_agent_count, render_payload,
    BannerInputs,
};
use core_paths::{load_stats, mem_dir};

fn main() -> ExitCode {
    let cwd = resolve_cwd();
    let project_root = core_paths::find_project_root(&cwd);
    let project_name = project_root
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let md = mem_dir(&cwd);
    let port: u16 = env::var("PROJECT_MEM_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(37778);

    let mem_dir_exists = md.exists();
    let stats = if mem_dir_exists {
        load_stats(&md)
    } else {
        None
    };
    let (fallback_sessions, fallback_observations) = if mem_dir_exists && stats.is_none() {
        count_fallback(&md)
    } else {
        (0, 0)
    };
    let has_project_md = mem_dir_exists && md.join("PROJECT.md").exists();
    let agent_count = if mem_dir_exists {
        load_agent_count(&md)
    } else {
        None
    };

    let plugin_root = resolve_plugin_root();
    let seed_cli_path = plugin_root.join("scripts").join("seed.cjs");
    let mcp_path = plugin_root.join("mcp").join("server.cjs");

    let settings_path = resolve_settings_path();
    let mcp_warning = check_mcp_registered(&settings_path, &mcp_path);

    let banner = build_banner(&BannerInputs {
        project_name,
        mem_dir_exists,
        stats,
        fallback_sessions,
        fallback_observations,
        has_project_md,
        agent_count,
        seed_cli_path,
        ui_port: port,
        mcp_warning,
    });

    println!("{}", render_payload(&banner));
    ExitCode::SUCCESS
}

/// `CLAUDE_CWD || PWD || cwd()` — mesma ordem do `.cjs`.
fn resolve_cwd() -> PathBuf {
    if let Ok(p) = env::var("CLAUDE_CWD") {
        return PathBuf::from(p);
    }
    if let Ok(p) = env::var("PWD") {
        return PathBuf::from(p);
    }
    env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// `CLAUDE_PLUGIN_ROOT || dirname(__filename)/..` — o `.cjs` usa
/// `__dirname`, aqui caímos num default `.` pois o binário pode rodar
/// de qualquer lugar. O valor é sempre exportado pelo Claude Code, então
/// na prática o env var está sempre presente.
fn resolve_plugin_root() -> PathBuf {
    env::var("CLAUDE_PLUGIN_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// `CLAUDE_CONFIG_DIR/settings.json || $HOME/.claude/settings.json`.
fn resolve_settings_path() -> PathBuf {
    let dir = env::var("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir().join(".claude"));
    dir.join("settings.json")
}

fn home_dir() -> PathBuf {
    env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/"))
}

// Uma referência pra manter clippy feliz com o import implícito de Path.
#[allow(dead_code)]
fn _touch(_p: &Path) {}
