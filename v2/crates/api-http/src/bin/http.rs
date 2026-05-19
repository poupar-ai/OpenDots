//! `pmem-http` — binário do servidor HTTP.
//!
//! - cwd = `process.cwd()`, mem_dir = `findProjectRoot(cwd)/.claude-project-memory`
//! - Porta: `$PROJECT_MEM_PORT` (default 37778)
//! - Bind: resolvido via `<memDir>/network.json`
//! - UI: `ui-next/dist/` (build via `cd ui-next && npm run build`).
//!   Sem dist, `/` retorna 503 com instrução.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use adapter_vault_file::FileVaultStore;
use api_http::{build_router, socket_addr_for, AppState, DEFAULT_PORT};
use core_paths::{find_project_root, read_scope_project_root, MEM_DIR_NAME};

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
    // `mem_dir_base` define ONDE a memória mora — sempre derivado do cwd
    // (git toplevel ou cwd cru), pra que `.claude-project-memory/` fique
    // sempre junto do binário. Não usar `project_root` aqui: quando o
    // project_root aponta pro pai (workspace), a memória fica perdida no
    // pai e some quando o usuário troca o escopo.
    let mem_dir_base = find_project_root(&cwd);
    let mem_dir = std::env::var("PROJECT_MEM_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| mem_dir_base.join(MEM_DIR_NAME));
    // `project_root` (escopo do agente — cwd que ele recebe ao ser spawnado)
    // tem 3 fontes em ordem de prioridade:
    //   1. `<mem_dir>/scope.json::project_root` — configurável pela UI.
    //   2. Smart default: se `cwd` é clone do plugin (tem `.claude-plugin/`),
    //      usa `cwd.parent()` — o clone é só armazenamento, o "projeto" de
    //      verdade é o workspace onde ele foi colocado.
    //   3. Fallback: `mem_dir_base` (cwd cru).
    // O smart default cobre a instalação típica (seed clona em
    // `<workspace>/claude-project-memory/`) sem exigir scope.json.
    let default_project_root = if cwd.join(".claude-plugin").is_dir() {
        cwd.parent().map(Path::to_path_buf).unwrap_or_else(|| mem_dir_base.clone())
    } else {
        mem_dir_base.clone()
    };
    let project_root = read_scope_project_root(&mem_dir).unwrap_or(default_project_root);

    // `ui-next/dist` mora **dentro do clone do plugin** (junto do binário),
    // NÃO no projeto-alvo. Quando o user ajusta `project_root` via UI pra
    // apontar pro workspace (ex.: `~/dev/paag`), procurar dist lá quebra
    // (some). Resolvemos via `mem_dir_base` (find_project_root(cwd)) que
    // é sempre o clone do plugin.
    let dist_candidate = mem_dir_base.join("ui-next").join("dist");
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

    // Loop de reconciliação: a cada 60s, varre `running` da queue e
    // detecta turns que passaram do TTL (default 5 min). Zumbi volta
    // pra `pending` com retries+1; se passou de max_retries, vai pro
    // `dead.jsonl`. Rodando em background, sobrevive ao serve loop.
    let queue_for_reconcile = std::sync::Arc::clone(&state.auto_queue);
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(60));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            let res = queue_for_reconcile.reconcile();
            if res.recycled > 0 || res.dead > 0 {
                eprintln!(
                    "reconcile: {} zumbi(s) reciclado(s), {} dead-letter",
                    res.recycled, res.dead
                );
            }
        }
    });

    // Cleanup periódico de subagents finished > 1h. Sem isso o map cresce
    // indefinidamente em uso pesado.
    let subagents_for_cleanup = std::sync::Arc::clone(&state.subagents);
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(60));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            let removed = subagents_for_cleanup.cleanup_finished(std::time::Duration::from_secs(3600));
            if removed > 0 {
                eprintln!("subagent cleanup: {removed} finished removidos");
            }
        }
    });

    let app = build_router(state);

    // Bind: por default TCP em $bind_host:$port. Env PROJECT_MEM_BIND_UNIX
    // sobrescreve pra Unix socket (endurece contra ataques same-LAN —
    // ninguém na rede consegue chegar). UI quebra em modo Unix-only
    // (browser não fala unix); pra ter UI + Unix simultâneos, rodar
    // reverse proxy (caddy/nginx) que faça TCP localhost → Unix.
    //
    // Limitação atual: axum 0.7 `serve()` só aceita TcpListener. Pra ter
    // ambos simultâneos no MESMO processo precisa upgrade pra axum 0.8
    // (breaking em ~100 handlers — fora de escopo aqui).
    #[cfg(unix)]
    if let Ok(unix_path) = std::env::var("PROJECT_MEM_BIND_UNIX") {
        if !unix_path.is_empty() {
            let _ = std::fs::remove_file(&unix_path);
            // Truque pra axum 0.7 aceitar Unix: criar TcpListener "fake"
            // não funciona. Em vez disso usamos hyper direto via accept
            // loop manual.
            return serve_unix(&unix_path, app).await;
        }
    }

    let tcp_listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("falha ao bind em {addr}: {e}");
            return ExitCode::FAILURE;
        }
    };
    eprintln!("pmem-http escutando em http://{addr}");
    if let Err(e) = axum::serve(tcp_listener, app).await {
        eprintln!("servidor caiu: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// Servir o axum app sobre um Unix socket. Implementação manual via
/// hyper-util (já é dep transitiva do axum 0.7). Aceita conexões em
/// loop, faz upgrade pra http1 por conexão e despacha pra tower service.
#[cfg(unix)]
async fn serve_unix(path: &str, app: axum::Router) -> ExitCode {
    use hyper::server::conn::http1;
    use hyper_util::rt::TokioIo;

    let listener = match tokio::net::UnixListener::bind(path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("falha ao bind unix://{path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    eprintln!("pmem-http escutando em unix://{path}");

    use tower::ServiceExt;
    loop {
        let (stream, _) = match listener.accept().await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("accept falhou: {e}");
                continue;
            }
        };
        let io = TokioIo::new(stream);
        let app = app.clone();
        tokio::spawn(async move {
            // Pra cada request, clona o router e usa oneshot (consome o
            // service). Axum 0.7 Router não é Clone-with-call direto;
            // esse pattern é o documentado no exemplo unix-domain-socket
            // do repo do axum.
            let service = hyper::service::service_fn(move |req| {
                let app = app.clone();
                async move { app.oneshot(req).await }
            });
            if let Err(e) = http1::Builder::new().serve_connection(io, service).await {
                eprintln!("conn erro: {e}");
            }
        });
    }
}
