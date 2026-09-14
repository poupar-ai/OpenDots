//! Subagents efêmeros e paralelos — inspirado em Hermes Agent.
//!
//! Hermes spawna subagents isolados pra workstreams paralelos, agente
//! pai delega N tasks sem que isso custe tokens dele (cada subagent
//! tem context próprio). Análogo aqui:
//!
//! - Pai chama POST /api/agents/subagent/spawn body `{prompt, tools?}`.
//! - Server spawna `claude -p` em background (não bloqueia request),
//!   captura stdout, atualiza `SubagentState`.
//! - Pai poll via GET /api/agents/subagent/:id pra ver `status` e
//!   `result` quando pronto.
//! - Batch: POST /spawn_batch dispara N em paralelo. GET /batch/:id
//!   agrega status.
//!
//! ## Isolamento
//!
//! Cada subagent é UM processo independente:
//! - Próprio cwd (= project_root do server)
//! - Próprio env (não herda task IDs do pai)
//! - Tools restritas (default: readonly Read/Grep/Glob)
//! - **Sem JWT de agent**: subagents não fazem handoff de volta — só
//!   executam task e retornam stdout. Se quiserem comunicar com o pai,
//!   é via response da task.
//!
//! ## Custo de tokens
//!
//! Pai NÃO vê o prompt nem o context do subagent. Só recebe o
//! resultado final (string). Isso é o "zero-context-cost" do Hermes:
//! workstream paralelo sem inflar a janela do pai.
//!
//! ## Cleanup
//!
//! Loop periódico (60s) remove subagents `done` com `finished_at` >
//! 1h atrás. Pais que não polled em tempo perdem o resultado —
//! aceitável (subagents são efêmeros por design).

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio::process::Command as TokioCommand;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SubagentStatus {
    Running,
    Done,
    Failed,
    TimedOut,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubagentState {
    pub id: String,
    pub parent_agent: String,
    pub prompt_preview: String,
    pub status: SubagentStatus,
    pub started_at_ms: u64,
    pub finished_at_ms: Option<u64>,
    pub duration_ms: Option<u64>,
    pub result: Option<String>,
    pub error: Option<String>,
    /// ID opcional do batch ao qual esse subagent pertence.
    pub batch_id: Option<String>,
}

/// Handle interno pra abort do task em background. Separado do
/// SubagentState pra não vazar `tokio::task::AbortHandle` (não-serde)
/// nas responses JSON.
struct SubagentHandle {
    state: Arc<Mutex<SubagentState>>,
    abort: tokio::task::AbortHandle,
}

/// Manager — guarda estado de todos os subagents vivos + finished
/// recentes. Thread-safe via Mutex<HashMap>. Acesso é cheap (ms).
pub struct SubagentManager {
    state: Mutex<HashMap<String, SubagentHandle>>,
}

impl SubagentManager {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(HashMap::new()),
        }
    }

    /// Lista IDs por batch — usado pelo endpoint de batch status.
    pub fn list_batch(&self, batch_id: &str) -> Vec<Arc<Mutex<SubagentState>>> {
        let map = self.state.lock().unwrap();
        map.values()
            .filter(|h| {
                h.state
                    .lock()
                    .unwrap()
                    .batch_id
                    .as_deref()
                    .map(|b| b == batch_id)
                    .unwrap_or(false)
            })
            .map(|h| h.state.clone())
            .collect()
    }

    /// Lookup individual.
    pub fn get(&self, id: &str) -> Option<Arc<Mutex<SubagentState>>> {
        self.state.lock().unwrap().get(id).map(|h| h.state.clone())
    }

    /// Conta vivos por pai (pra rate limit por agent).
    pub fn count_running_for(&self, parent: &str) -> usize {
        let map = self.state.lock().unwrap();
        map.values()
            .filter(|h| {
                let st = h.state.lock().unwrap();
                st.parent_agent == parent && st.status == SubagentStatus::Running
            })
            .count()
    }

    /// Lista subagents filtrados por pai (opcional). Útil pro
    /// SubagentPanel da UI — pega visão geral.
    pub fn list(&self, parent_filter: Option<&str>) -> Vec<Arc<Mutex<SubagentState>>> {
        let map = self.state.lock().unwrap();
        let mut out: Vec<_> = map
            .values()
            .filter(|h| {
                parent_filter
                    .map(|p| h.state.lock().unwrap().parent_agent == p)
                    .unwrap_or(true)
            })
            .map(|h| h.state.clone())
            .collect();
        // Mais recente primeiro (started_at desc).
        out.sort_by(|a, b| {
            b.lock()
                .unwrap()
                .started_at_ms
                .cmp(&a.lock().unwrap().started_at_ms)
        });
        out
    }

    /// Cancela subagent em execução. Aborta o background task e marca
    /// status como Cancelled. Retorna `Ok` se cancelou, `Err` se já
    /// estava finished ou se ID não existe.
    pub fn cancel(&self, id: &str) -> Result<(), &'static str> {
        let map = self.state.lock().unwrap();
        let handle = map.get(id).ok_or("subagent não encontrado")?;
        let mut st = handle.state.lock().unwrap();
        if st.status != SubagentStatus::Running {
            return Err("subagent já finalizado");
        }
        handle.abort.abort();
        st.status = SubagentStatus::Cancelled;
        st.finished_at_ms = Some(unix_now_ms());
        st.duration_ms = Some(
            st.finished_at_ms
                .unwrap()
                .saturating_sub(st.started_at_ms),
        );
        st.error = Some("cancelado por DELETE /api/agents/subagent/:id".into());
        Ok(())
    }

    /// Remove subagents `done|failed|timed_out|cancelled` com finished > `max_age`.
    /// Não toca em `running`. Retorna count removido.
    pub fn cleanup_finished(&self, max_age: Duration) -> usize {
        let cutoff = unix_now_ms().saturating_sub(max_age.as_millis() as u64);
        let mut map = self.state.lock().unwrap();
        let before = map.len();
        map.retain(|_, h| {
            let st = h.state.lock().unwrap();
            match st.status {
                SubagentStatus::Running => true,
                _ => st.finished_at_ms.map(|t| t > cutoff).unwrap_or(true),
            }
        });
        before - map.len()
    }

    /// Spawn — registra o state e dispara processo em background. Não
    /// bloqueia. Retorna o ID imediatamente.
    pub fn spawn(
        self: &Arc<Self>,
        parent_agent: &str,
        prompt: String,
        cwd: PathBuf,
        model: Option<String>,
        allowed_tools: Vec<String>,
        timeout: Duration,
        batch_id: Option<String>,
    ) -> String {
        let id = new_subagent_id();
        let prompt_preview: String = prompt.chars().take(120).collect();
        let started_at_ms = unix_now_ms();
        let state = Arc::new(Mutex::new(SubagentState {
            id: id.clone(),
            parent_agent: parent_agent.to_string(),
            prompt_preview,
            status: SubagentStatus::Running,
            started_at_ms,
            finished_at_ms: None,
            duration_ms: None,
            result: None,
            error: None,
            batch_id,
        }));

        // Background: spawn claude CLI, captura stdout, atualiza state.
        let state_for_task = state.clone();
        let id_for_log = id.clone();
        let join = tokio::spawn(async move {
            let started = Instant::now();
            // [ADR-001] Claude CLI hardcoded — vide docs/adr-001-multi-modelo.md.
            let mut args = vec!["-p".to_string(), "--output-format".to_string(), "text".to_string()];
            if let Some(m) = model {
                args.push("--model".into());
                args.push(m);
            }
            if !allowed_tools.is_empty() {
                args.push("--allowed-tools".into());
                args.push(allowed_tools.join(","));
            }
            args.push("--".into());
            args.push(prompt);

            let mut cmd = TokioCommand::new("claude");
            cmd.args(&args)
                .current_dir(&cwd)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .env("CLAUDE_POUPAR_SUBAGENT", "1")
                .env("CLAUDE_POUPAR_PARENT_AGENT", &state_for_task.lock().unwrap().parent_agent)
                .kill_on_drop(true);

            let child = match cmd.spawn() {
                Ok(c) => c,
                Err(e) => {
                    let mut st = state_for_task.lock().unwrap();
                    st.status = SubagentStatus::Failed;
                    st.error = Some(format!("spawn failed: {e}"));
                    st.finished_at_ms = Some(unix_now_ms());
                    st.duration_ms = Some(started.elapsed().as_millis() as u64);
                    eprintln!("subagent {id_for_log} spawn falhou: {e}");
                    return;
                }
            };

            // Aguarda com timeout. Se estourar, mata o processo.
            let wait_fut = wait_with_output(child);
            let outcome = tokio::select! {
                r = wait_fut => Some(r),
                _ = tokio::time::sleep(timeout) => None,
            };
            let elapsed_ms = started.elapsed().as_millis() as u64;
            let mut st = state_for_task.lock().unwrap();
            st.finished_at_ms = Some(unix_now_ms());
            st.duration_ms = Some(elapsed_ms);
            match outcome {
                Some(Ok((stdout, stderr, code))) => {
                    if code.unwrap_or(-1) == 0 {
                        st.status = SubagentStatus::Done;
                        st.result = Some(stdout);
                        if !stderr.is_empty() {
                            st.error = Some(stderr.chars().take(500).collect());
                        }
                    } else {
                        st.status = SubagentStatus::Failed;
                        st.error = Some(format!(
                            "exit code {}: {}",
                            code.unwrap_or(-1),
                            stderr.chars().take(500).collect::<String>()
                        ));
                        if !stdout.is_empty() {
                            st.result = Some(stdout);
                        }
                    }
                }
                Some(Err(e)) => {
                    st.status = SubagentStatus::Failed;
                    st.error = Some(format!("wait failed: {e}"));
                }
                None => {
                    st.status = SubagentStatus::TimedOut;
                    st.error = Some(format!("timeout após {}s", timeout.as_secs()));
                }
            }
        });

        // Inserir handle no map APÓS spawn — abort handle do task que
        // acabou de subir. Cancel via abort() vai matar o task (e o
        // child via kill_on_drop).
        let abort = join.abort_handle();
        self.state.lock().unwrap().insert(
            id.clone(),
            SubagentHandle { state, abort },
        );

        id
    }
}

impl Default for SubagentManager {
    fn default() -> Self {
        Self::new()
    }
}

async fn wait_with_output(
    mut child: tokio::process::Child,
) -> Result<(String, String, Option<i32>), std::io::Error> {
    let mut stdout_buf = String::new();
    let mut stderr_buf = String::new();
    if let Some(mut out) = child.stdout.take() {
        out.read_to_string(&mut stdout_buf).await?;
    }
    if let Some(mut err) = child.stderr.take() {
        err.read_to_string(&mut stderr_buf).await?;
    }
    let status = child.wait().await?;
    Ok((stdout_buf, stderr_buf, status.code()))
}

fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn new_subagent_id() -> String {
    use std::time::Instant;
    // sa_<base36 ts>_<random>
    let ts = unix_now_ms();
    let stack = &ts as *const _ as u64;
    let nanos = Instant::now().elapsed().as_nanos() as u64;
    let mut rand = ts ^ stack ^ nanos.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let mut suffix = String::with_capacity(8);
    for _ in 0..8 {
        let d = (rand % 36) as u32;
        rand /= 36;
        suffix.push(std::char::from_digit(d, 36).unwrap_or('0'));
    }
    format!("sa_{:x}_{}", ts, suffix)
}

pub fn new_batch_id() -> String {
    let ts = unix_now_ms();
    format!("bat_{:x}", ts)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Insere state dummy no map (com abort handle de no-op task).
    /// Tests não fazem spawn real porque não tem `claude` CLI.
    fn dummy_insert(mgr: &SubagentManager, id: &str, st: SubagentState) {
        let join = tokio::spawn(async {});
        let abort = join.abort_handle();
        mgr.state.lock().unwrap().insert(
            id.to_string(),
            SubagentHandle {
                state: Arc::new(Mutex::new(st)),
                abort,
            },
        );
    }

    #[tokio::test]
    async fn manager_tracks_state() {
        let mgr = Arc::new(SubagentManager::new());
        let st = SubagentState {
            id: "sa_test".into(),
            parent_agent: "manager".into(),
            prompt_preview: "test".into(),
            status: SubagentStatus::Running,
            started_at_ms: 0,
            finished_at_ms: None,
            duration_ms: None,
            result: None,
            error: None,
            batch_id: Some("bat_1".into()),
        };
        dummy_insert(&mgr, "sa_test", st);
        assert!(mgr.get("sa_test").is_some());
        assert_eq!(mgr.count_running_for("manager"), 1);
        assert_eq!(mgr.list_batch("bat_1").len(), 1);
        assert_eq!(mgr.list_batch("bat_X").len(), 0);
        // list(None) inclui tudo; list(Some("manager")) só do pai.
        assert_eq!(mgr.list(None).len(), 1);
        assert_eq!(mgr.list(Some("manager")).len(), 1);
        assert_eq!(mgr.list(Some("outro")).len(), 0);
    }

    #[tokio::test]
    async fn cancel_marks_status_and_aborts() {
        let mgr = SubagentManager::new();
        let st = SubagentState {
            id: "sa_cancel".into(),
            parent_agent: "x".into(),
            prompt_preview: "".into(),
            status: SubagentStatus::Running,
            started_at_ms: unix_now_ms(),
            finished_at_ms: None,
            duration_ms: None,
            result: None,
            error: None,
            batch_id: None,
        };
        dummy_insert(&mgr, "sa_cancel", st);
        assert!(mgr.cancel("sa_cancel").is_ok());
        let snap = mgr.get("sa_cancel").unwrap();
        assert_eq!(snap.lock().unwrap().status, SubagentStatus::Cancelled);
        // Segunda cancel falha (já não está running).
        assert!(mgr.cancel("sa_cancel").is_err());
        // ID inexistente falha.
        assert!(mgr.cancel("nao_existe").is_err());
    }

    #[tokio::test]
    async fn cleanup_removes_old_finished() {
        let mgr = SubagentManager::new();
        let now = unix_now_ms();
        let old = SubagentState {
            id: "sa_old".into(),
            parent_agent: "x".into(),
            prompt_preview: "".into(),
            status: SubagentStatus::Done,
            started_at_ms: now - 3_700_000,
            finished_at_ms: Some(now - 3_700_000),
            duration_ms: Some(100),
            result: Some("done".into()),
            error: None,
            batch_id: None,
        };
        let fresh = SubagentState {
            id: "sa_fresh".into(),
            parent_agent: "x".into(),
            prompt_preview: "".into(),
            status: SubagentStatus::Done,
            started_at_ms: now - 1000,
            finished_at_ms: Some(now - 1000),
            duration_ms: Some(100),
            result: Some("done".into()),
            error: None,
            batch_id: None,
        };
        let running = SubagentState {
            id: "sa_run".into(),
            parent_agent: "x".into(),
            prompt_preview: "".into(),
            status: SubagentStatus::Running,
            started_at_ms: now - 10_000_000,
            finished_at_ms: None,
            duration_ms: None,
            result: None,
            error: None,
            batch_id: None,
        };
        dummy_insert(&mgr, "sa_old", old);
        dummy_insert(&mgr, "sa_fresh", fresh);
        dummy_insert(&mgr, "sa_run", running);

        let removed = mgr.cleanup_finished(Duration::from_secs(3600));
        assert_eq!(removed, 1);
        assert!(mgr.get("sa_old").is_none());
        assert!(mgr.get("sa_fresh").is_some());
        assert!(mgr.get("sa_run").is_some());
    }

    #[test]
    fn subagent_id_format() {
        let id = new_subagent_id();
        assert!(id.starts_with("sa_"));
        // formato: sa_<hex>_<8 chars base36>
        let parts: Vec<&str> = id.split('_').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[2].len(), 8);
    }
}
