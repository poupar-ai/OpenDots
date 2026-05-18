//! `pmem-http` — binário do servidor HTTP.
//!
//! Paridade com `plugin/ui/server.cjs`:
//! - cwd = `process.cwd()`, mem_dir = `findProjectRoot(cwd)/.claude-project-memory`
//! - Porta: `$PROJECT_MEM_PORT` (default 37778)
//! - Bind: resolvido via `<memDir>/network.json`
//! - UI: `include_str!("../../../../plugin/ui/index.html")` (resolvido em build)
//!   + fallback pra `ui-next/dist/` se existir

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use adapter_vault_file::FileVaultStore;
use api_http::{build_router, socket_addr_for, AppState, DEFAULT_PORT};
use core_paths::{find_project_root, read_scope_project_root, MEM_DIR_NAME};

const INDEX_HTML: &str = include_str!("../../../../../plugin/ui/index.html");

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> ExitCode {
    let port: u16 = std::env::var("PROJECT_MEM_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_PORT);

    let cwd = std::env::var("CLAUDE_CWD")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("PWD").map(PathBuf::from))
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    // `default_project_root` é derivado do cwd (git toplevel ou cwd cru).
    // Ele *sempre* define o `mem_dir` — assim a memória mora junto do
    // binário/clone, não importa o escopo configurado.
    let default_project_root = find_project_root(&cwd);
    // `PROJECT_MEM_DIR` (env) ainda funciona como override hard se alguém
    // quiser apontar pra um cofre alternativo. Caso normal: derivado do
    // default_project_root.
    let mem_dir = std::env::var("PROJECT_MEM_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_project_root.join(MEM_DIR_NAME));
    // `project_root` (escopo do agente — cwd que ele recebe ao ser spawnado)
    // tem 3 fontes em ordem de prioridade:
    //   1. `<mem_dir>/scope.json::project_root` — configurável pela UI.
    //   2. ENV `CLAUDE_CWD`/`PWD` (via `cwd`) — comportamento legado.
    //   3. `default_project_root` (find_project_root do cwd) — fallback.
    // (1) > (2)+(3) porque a UI é a forma do usuário dizer explicitamente
    // "rode o agente apontando pra cá", sem mexer em env.
    let project_root = read_scope_project_root(&mem_dir).unwrap_or(default_project_root);

    // `DIST_DIR` = `<plugin_dir>/../../ui-next/dist`. Aqui não temos
    // `__dirname`; tentamos resolver via `CARGO_MANIFEST_DIR` em build
    // vs runtime. Em runtime default: relativo ao CWD do projeto.
    let dist_candidate = project_root.join("ui-next").join("dist");
    let dist_dir = if dist_candidate.join("index.html").exists() {
        Some(dist_candidate)
    } else {
        None
    };

    // `plugin_dir` aponta pra raiz da pasta `plugin/` (onde ficam `seeds/`,
    // `pipelines/`, `best-practices/`, etc). Node resolve via `__dirname/..`;
    // aqui via env `PROJECT_MEM_PLUGIN_DIR` com fallback compile-time.
    let plugin_dir = std::env::var("PROJECT_MEM_PLUGIN_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../plugin")));

    let vault = Arc::new(FileVaultStore::new());
    let state = AppState::new(
        mem_dir,
        project_root,
        plugin_dir,
        port,
        INDEX_HTML.to_string(),
        dist_dir,
        vault,
    );
    let addr: SocketAddr = match socket_addr_for(&state.bind_host, port) {
        Some(a) => a,
        None => {
            eprintln!("bind host inválido: {}", state.bind_host);
            return ExitCode::FAILURE;
        }
    };

    let app = build_router(state);
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("falha ao bind em {addr}: {e}");
            return ExitCode::FAILURE;
        }
    };
    eprintln!("pmem-http escutando em http://{addr}");
    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("servidor caiu: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
