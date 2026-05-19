//! Runner que processa turns enfileirados pela `AgentQueue`.
//!
//! Paridade com `plugin/ui/server.cjs::runHeadlessTurn` (linha 905):
//! - carrega manifest + agent config
//! - compõe prompt usando mini_context + history
//! - emite `bus.delivered` pra cada trace_id na batch
//! - spawn `claude -p --output-format text` com tools/env corretos
//! - persiste resposta como `agent` em chat e log de `response`
//! - em erro, grava `agent_logs` com stage `spawn|exit|proc`
//!
//! Sem isso, a fila aceitava enqueue mas o turn nunca rodava — o AppState
//! antigo usava um runner placeholder no-op (TODO F4-05f-5).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Instant;

use core_ports::VaultStore;
use core_queue::{BoxFut, Runner};
use domain_bus::SystemClock as BusSystemClock;
use domain_humans::{Clock as HumansClockTrait, SystemClock as HumansSystemClock};
use serde_json::{json, Value};
use tokio::process::Command as TokioCommand;

use crate::agents_routes::append_agent_log;
use crate::chat_routes::{append_chat, read_chat};
use crate::chat_send_routes::{
    build_agent_env, build_allowed_tools, build_disallowed_tools, compose_prompt,
};

/// Constrói o runner pronto pra entregar ao `AgentQueueConfig::new`.
/// O closure captura clones dos paths/port/vault — não há ciclo com
/// `AppState`/`AgentQueue`.
pub fn build_runner(
    mem_dir: PathBuf,
    project_root: PathBuf,
    plugin_dir: PathBuf,
    port: u16,
    vault: Arc<dyn VaultStore>,
    trace_index: Arc<domain_bus::TraceIndex>,
    search_index: Option<Arc<core_storage::SearchIndex>>,
) -> Runner {
    Arc::new(move |to, from, message, traces| {
        let mem_dir = mem_dir.clone();
        let project_root = project_root.clone();
        let plugin_dir = plugin_dir.clone();
        let vault = Arc::clone(&vault);
        let trace_index = Arc::clone(&trace_index);
        let search_index = search_index.clone();
        Box::pin(async move {
            run_headless_turn(
                &mem_dir,
                &project_root,
                &plugin_dir,
                port,
                &vault,
                &trace_index,
                search_index.as_ref(),
                &to,
                &from,
                &message,
                &traces,
            )
            .await;
        }) as BoxFut
    })
}

/// Núcleo: paridade com `runHeadlessTurn(agent, fromAgent, userMessage, traceIds)`.
async fn run_headless_turn(
    mem_dir: &Path,
    project_root: &Path,
    plugin_dir: &Path,
    port: u16,
    vault: &Arc<dyn VaultStore>,
    trace_index: &Arc<domain_bus::TraceIndex>,
    search_index: Option<&Arc<core_storage::SearchIndex>>,
    agent: &str,
    from_agent: &str,
    user_message: &str,
    trace_ids: &[String],
) {
    let _ = plugin_dir; // reservado pra `integrity.snapshot` num próximo passo

    let Some(manifest) = domain_agents::load_manifest(mem_dir) else {
        return;
    };
    let agent_cfg = match manifest
        .get("agents")
        .and_then(|a| a.get(agent))
        .cloned()
    {
        Some(c) => c,
        None => return,
    };

    let ctx_file = agent_cfg
        .get("context_file")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let mini_context = domain_agents::load_mini_context(mem_dir, ctx_file).unwrap_or_default();
    let history = read_chat(mem_dir, agent);

    // `human=None` no fluxo da fila (auto-handoff entre agentes).
    let prompt = compose_prompt(
        port,
        agent,
        &agent_cfg,
        &mini_context,
        user_message,
        &history,
        None,
        Some(from_agent),
    );

    let start_ts = HumansSystemClock.now_iso();
    append_agent_log(
        mem_dir,
        agent,
        &json!({
            "ts": start_ts,
            "type": "request",
            "stage": "auto_handoff",
            "from_agent": from_agent,
            "prompt_chars": prompt.len(),
            "user_chars": user_message.len(),
            "history_turns": history.len(),
            "model": agent_cfg.get("model").cloned().unwrap_or(Value::Null),
        }),
    );

    // bus: `delivered` pra cada trace_id da batch (paridade L929-931).
    // Emit + record no índice — mantém trace_index sincronizado.
    for tid in trace_ids.iter().filter(|t| !t.is_empty()) {
        if let Some(ev) = domain_bus::emit(
            mem_dir,
            &json!({
                "type": "delivered",
                "trace_id": tid,
                "to": agent,
                "from": from_agent,
            }),
            &BusSystemClock,
        ) {
            trace_index.record(&ev);
        }
    }

    let mut args: Vec<String> = vec!["-p".into(), "--output-format".into(), "text".into()];
    if let Some(m) = domain_agents::resolve_cli_model(
        agent_cfg.get("model").unwrap_or(&Value::Null),
    ) {
        args.push("--model".into());
        args.push(m);
    }
    let allowed = build_allowed_tools(&agent_cfg);
    if !allowed.is_empty() {
        args.push("--allowed-tools".into());
        args.push(allowed.join(","));
    }
    let disallowed = build_disallowed_tools();
    if !disallowed.is_empty() {
        args.push("--disallowed-tools".into());
        args.push(disallowed.join(","));
    }
    args.push("--".into());
    args.push(prompt);

    // [ADR-001] Claude CLI hardcoded — vide docs/adr-001-multi-modelo.md
    // pra contexto e plano de migração quando vier multi-provider.
    let mut cmd = TokioCommand::new("claude");
    cmd.args(&args)
        .current_dir(project_root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("CLAUDE_PROJECT_MEM_SUBAGENT", "1")
        .env("CLAUDE_PROJECT_MEM_AGENT", agent)
        .kill_on_drop(true);
    // Mesmo flow do chat_send_handler — token Ed25519 quando vault
    // permite. Auto-handoff entre agentes precisa autenticar igual
    // ao chat originado pelo humano.
    if let Some(token) = crate::agent_keys::mint_token_for(agent, vault, mem_dir) {
        cmd.env("PROJECT_MEM_AGENT_TOKEN", token);
    }
    // `plugin_dir/agent-bin` no PATH — agente chama `pmem handoff ...`
    // sem expor $VAR na linha de comando (sandbox `simple_expansion` do
    // Claude Code rejeita qualquer Bash com expansão de variável). O
    // wrapper `pmem` lê o token do env internamente.
    let agent_bin = plugin_dir.join("agent-bin");
    let path_now = std::env::var("PATH").unwrap_or_default();
    cmd.env("PATH", format!("{}:{}", agent_bin.display(), path_now));
    cmd.env("PROJECT_MEM_PORT", port.to_string());
    for (k, v) in build_agent_env(mem_dir, vault, agent, &agent_cfg) {
        cmd.env(k, v);
    }

    let started = Instant::now();
    let output = cmd.output().await;
    let duration_ms = started.elapsed().as_millis() as u64;
    let end_ts = HumansSystemClock.now_iso();

    match output {
        Err(e) => {
            append_agent_log(
                mem_dir,
                agent,
                &json!({
                    "ts": end_ts,
                    "type": "error",
                    "stage": "spawn",
                    "message": e.to_string(),
                    "duration_ms": duration_ms,
                }),
            );
        }
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
            let code = out.status.code();
            if out.status.success() && !stdout.is_empty() {
                append_chat(
                    mem_dir,
                    agent,
                    &json!({
                        "ts": end_ts,
                        "role": "agent",
                        "content": stdout.clone(),
                        "duration_ms": duration_ms,
                        "from": agent,
                    }),
                    search_index,
                );
                append_agent_log(
                    mem_dir,
                    agent,
                    &json!({
                        "ts": end_ts,
                        "type": "response",
                        "to_agent": from_agent,
                        "response_chars": stdout.len(),
                        "duration_ms": duration_ms,
                        "code": code,
                    }),
                );
            } else {
                let snippet: String = stderr.chars().take(500).collect();
                append_agent_log(
                    mem_dir,
                    agent,
                    &json!({
                        "ts": end_ts,
                        "type": "error",
                        "stage": "exit",
                        "code": code,
                        "stderr": snippet,
                        "duration_ms": duration_ms,
                    }),
                );
            }
        }
    }
}
