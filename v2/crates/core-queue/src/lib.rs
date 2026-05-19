//! Motor genérico de fila de turns por agente.
//!
//! Port direto de `plugin/scripts/queue.cjs`. Mantém estado de
//! `running` + `pending` + janela deslizante de rate-limit e drena em
//! lote por agente — múltiplos pedidos pro mesmo destinatário viram uma
//! única chamada do `runner` quando chegam em rajada.
//!
//! ## Filosofia
//!
//! O motor NÃO sabe de spawn de processos, bus, logging ou persistência.
//! Tudo externo entra via `runner` (chamada async) e hooks opcionais.
//! Isso permite reuso (auto-turns do chat, shadow agents, workers
//! futuros) e habilita testes determinísticos sem mock de runtime.
//!
//! ## Diferenças mínimas vs `.cjs`
//!
//! - `setImmediate(drain)` → `tokio::spawn(drain_one)`. Contrato igual
//!   ("enfileira e processa depois"); em Rust exige `Arc<Self>` clone.
//! - `Date.now()` → `Clock` injetável (default `SystemClock` usa
//!   `Instant::now`). Tests usam `ManualClock` para rate-limit
//!   determinístico.
//! - Runners retornam `()` (Node tem `try/catch` pra swallow; Rust
//!   documenta que runner não pode entrar em panic).
//! - `_running`/`_pending` expostos no .cjs pra introspecção não têm
//!   equivalente — use `stats()` / `is_running()` / `pending_size()`.

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

// ─── Tipos públicos ──────────────────────────────────────────────────

/// Future retornado pelo `runner`. `'static` para poder ser spawnado.
pub type BoxFut = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

/// Callback que executa efetivamente o turn — equivalente ao parâmetro
/// `runner` no `.cjs`. Assinatura: `(to_agent, from_label, combined, trace_ids)`.
pub type Runner =
    Arc<dyn Fn(String, String, String, Vec<String>) -> BoxFut + Send + Sync + 'static>;

/// Hook sync opcional. Se panic, propaga — diferente do `.cjs` que tem
/// `try {} catch {}`. Hooks devem ser total.
pub type Hook<E> = Arc<dyn Fn(E) + Send + Sync + 'static>;

#[derive(Debug, Clone)]
pub struct ThrottleEvent {
    pub to_agent: String,
    pub from_agent: String,
    pub trace_id: Option<String>,
    pub reason: &'static str,
    pub rate_max: usize,
    pub rate_window_ms: u64,
}

#[derive(Debug, Clone)]
pub struct EnqueueEvent {
    pub to_agent: String,
    pub from_agent: String,
    pub trace_id: Option<String>,
    pub size: usize,
}

#[derive(Debug, Clone)]
pub struct DrainStartEvent {
    pub to_agent: String,
    pub batch_size: usize,
    pub trace_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DrainEndEvent {
    pub to_agent: String,
}

/// Item movido pra dead-letter — falhou (ou ficou zumbi) `max_retries`
/// vezes. Caller pode logar/alertar via hook.
#[derive(Debug, Clone)]
pub struct DeadLetterEvent {
    pub to_agent: String,
    pub from_agent: String,
    pub trace_id: Option<String>,
    pub retries: u8,
    pub reason: &'static str,
}

#[derive(Debug, Clone, Default)]
pub struct ReconcileResult {
    /// Quantos entries voltaram pra `pending` (retries+1).
    pub recycled: usize,
    /// Quantos entries foram pro `dead.jsonl`.
    pub dead: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueStats {
    pub running: Vec<String>,
    pub pending: HashMap<String, usize>,
    pub rate_used: usize,
    pub rate_max: usize,
    pub rate_window_ms: u64,
}

// ─── Clock ───────────────────────────────────────────────────────────

/// Abstração de relógio para habilitar rate-limit determinístico em
/// tests. Unidade: milissegundos (paridade com `Date.now()` do `.cjs`).
pub trait Clock: Send + Sync + 'static {
    fn now_millis(&self) -> u64;
}

/// Relógio baseado em `Instant::now()`. Monotônico; origem no primeiro
/// `now_millis` (não é epoch, mas como usamos só deltas é equivalente).
pub struct SystemClock {
    origin: Instant,
}

impl SystemClock {
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn now_millis(&self) -> u64 {
        self.origin.elapsed().as_millis() as u64
    }
}

/// Clock manualmente avançável (para testes). Thread-safe via
/// `AtomicU64`.
pub struct ManualClock {
    now: AtomicU64,
}

impl ManualClock {
    pub fn new() -> Self {
        Self {
            now: AtomicU64::new(0),
        }
    }

    pub fn advance(&self, ms: u64) {
        self.now.fetch_add(ms, Ordering::SeqCst);
    }

    pub fn set(&self, ms: u64) {
        self.now.store(ms, Ordering::SeqCst);
    }
}

impl Default for ManualClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for ManualClock {
    fn now_millis(&self) -> u64 {
        self.now.load(Ordering::SeqCst)
    }
}

// ─── Config ──────────────────────────────────────────────────────────

/// Parâmetros de criação. `runner` é obrigatório; demais têm defaults
/// que batem com o `.cjs` (rate_max=40, rate_window_ms=60_000).
pub struct AgentQueueConfig {
    pub runner: Runner,
    pub rate_max: usize,
    pub rate_window: Duration,
    pub on_throttle: Option<Hook<ThrottleEvent>>,
    pub on_enqueue: Option<Hook<EnqueueEvent>>,
    pub on_drain_start: Option<Hook<DrainStartEvent>>,
    pub on_drain_end: Option<Hook<DrainEndEvent>>,
    pub clock: Arc<dyn Clock + Send + Sync + 'static>,
    /// Pasta onde persistir `state.json` + `dead.jsonl`. None = só
    /// in-memory (comportamento legado — pending some em restart).
    pub state_dir: Option<PathBuf>,
    /// Tempo máximo que um turn pode ficar em `running` antes de ser
    /// considerado zumbi (processo morreu, etc.). reconcile() devolve
    /// zumbis pra pending com retries+1.
    pub turn_ttl: Duration,
    /// Após N tentativas, entry vai pro `dead.jsonl` em vez de voltar
    /// pra pending.
    pub max_retries: u8,
    /// Idade máxima de uma mensagem em `pending` — passou disso e ainda
    /// não foi pro runner, vai pro dead.jsonl com reason="expired".
    /// Cobre o caso "intent tempo-crítico ficou esperando demais
    /// porque agente alvo estava ocupado/inativo".
    pub message_max_age: Duration,
    /// Hook chamado quando um item é movido pra dead-letter.
    pub on_dead_letter: Option<Hook<DeadLetterEvent>>,
}

impl AgentQueueConfig {
    /// Cria config com defaults do `.cjs` (rate_max=40, 60s, sem hooks,
    /// SystemClock). Persistência off (state_dir=None).
    pub fn new(runner: Runner) -> Self {
        Self {
            runner,
            rate_max: 40,
            rate_window: Duration::from_secs(60),
            on_throttle: None,
            on_enqueue: None,
            on_drain_start: None,
            on_drain_end: None,
            clock: Arc::new(SystemClock::new()),
            state_dir: None,
            turn_ttl: Duration::from_secs(300), // 5 min
            max_retries: 3,
            message_max_age: Duration::from_secs(3600), // 1h
            on_dead_letter: None,
        }
    }
}

// ─── Estado interno ──────────────────────────────────────────────────

/// Prioridade do entry. Menor número = maior prioridade. Default
/// `Normal`. Inserção em pending mantém ordem por prioridade (estável
/// dentro de cada nível). Auto-escalações de bombeiro = High; humano
/// no chat = Normal; auto-handoff entre agentes = Low.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Priority {
    High,
    Normal,
    Low,
}

impl Default for Priority {
    fn default() -> Self {
        Priority::Normal
    }
}

impl Priority {
    fn as_u8(self) -> u8 {
        match self {
            Priority::High => 0,
            Priority::Normal => 1,
            Priority::Low => 2,
        }
    }
    fn from_u8(v: u8) -> Self {
        match v {
            0 => Priority::High,
            2 => Priority::Low,
            _ => Priority::Normal,
        }
    }
    /// Parse de string vinda do HTTP — `"high"|"normal"|"low"` (case
    /// insensitive). Default Normal.
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "high" | "urgent" | "h" => Priority::High,
            "low" | "l" => Priority::Low,
            _ => Priority::Normal,
        }
    }
}

#[derive(Clone)]
struct Entry {
    from_agent: String,
    message: String,
    trace_id: Option<String>,
    /// Unix epoch ms — quando o item foi enfileirado (ou re-enfileirado
    /// pela reconciliação).
    enqueued_at_ms: u64,
    /// Quantas vezes esse item já tentou rodar e falhou (zumbi). 0 na
    /// primeira tentativa.
    retries: u8,
    /// Prioridade da entrega.
    priority: Priority,
}

#[derive(Clone)]
struct RunningMeta {
    from_agent: String,
    trace_id: Option<String>,
    /// Unix epoch ms — quando o turn começou. reconcile() compara
    /// `now() - started_at_ms` com `turn_ttl`.
    started_at_ms: u64,
    /// Carregado do entry no momento do acquire — se o turn for
    /// declarado zumbi, esse é o `retries` atual (será incrementado).
    retries: u8,
}

#[derive(Default)]
struct State {
    running: HashMap<String, RunningMeta>,
    pending: HashMap<String, VecDeque<Entry>>,
    rate_stamps: VecDeque<u64>,
}

// ─── Persistência (state_dir setado) ─────────────────────────────────

#[derive(Serialize, Deserialize)]
struct PersistedEntry {
    from_agent: String,
    message: String,
    trace_id: Option<String>,
    enqueued_at_ms: u64,
    retries: u8,
    #[serde(default)]
    priority: u8,
}

#[derive(Serialize, Deserialize)]
struct PersistedRunning {
    from_agent: String,
    trace_id: Option<String>,
    started_at_ms: u64,
    retries: u8,
}

#[derive(Serialize, Deserialize, Default)]
struct PersistedState {
    pending: HashMap<String, Vec<PersistedEntry>>,
    running: HashMap<String, PersistedRunning>,
}

fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn state_file(state_dir: &Path) -> PathBuf {
    state_dir.join("state.json")
}

fn dead_file(state_dir: &Path) -> PathBuf {
    state_dir.join("dead.jsonl")
}

/// Snapshot do `State` → `state.json` (atomic write via tmp+rename).
/// Falhas silenciadas (logs ficariam ruidosos demais com cada enqueue);
/// se disco quebra, próximo restart cai no estado vazio — comportamento
/// pré-persistência. Não é crítico pra correção.
fn persist_state(state_dir: &Path, st: &State) {
    let p = PersistedState {
        pending: st
            .pending
            .iter()
            .map(|(k, q)| {
                (
                    k.clone(),
                    q.iter()
                        .map(|e| PersistedEntry {
                            from_agent: e.from_agent.clone(),
                            message: e.message.clone(),
                            trace_id: e.trace_id.clone(),
                            enqueued_at_ms: e.enqueued_at_ms,
                            retries: e.retries,
                            priority: e.priority.as_u8(),
                        })
                        .collect(),
                )
            })
            .collect(),
        running: st
            .running
            .iter()
            .map(|(k, m)| {
                (
                    k.clone(),
                    PersistedRunning {
                        from_agent: m.from_agent.clone(),
                        trace_id: m.trace_id.clone(),
                        started_at_ms: m.started_at_ms,
                        retries: m.retries,
                    },
                )
            })
            .collect(),
    };
    let Ok(json) = serde_json::to_string_pretty(&p) else {
        return;
    };
    let _ = fs::create_dir_all(state_dir);
    let tmp = state_dir.join("state.json.tmp");
    if fs::write(&tmp, json).is_ok() {
        let _ = fs::rename(&tmp, state_file(state_dir));
    }
}

/// Lê `state.json` se existir. Tudo que falhar (file ausente, JSON
/// corrupto, schema antigo) cai no `State::default()` — o pior que
/// acontece é perder a queue (que é o estado pré-persistência).
fn restore_state(state_dir: &Path) -> State {
    let content = match fs::read_to_string(state_file(state_dir)) {
        Ok(c) => c,
        Err(_) => return State::default(),
    };
    let parsed: PersistedState = match serde_json::from_str(&content) {
        Ok(p) => p,
        Err(_) => return State::default(),
    };
    State {
        pending: parsed
            .pending
            .into_iter()
            .map(|(k, v)| {
                (
                    k,
                    v.into_iter()
                        .map(|e| Entry {
                            from_agent: e.from_agent,
                            message: e.message,
                            trace_id: e.trace_id,
                            enqueued_at_ms: e.enqueued_at_ms,
                            retries: e.retries,
                            priority: Priority::from_u8(e.priority),
                        })
                        .collect(),
                )
            })
            .collect(),
        running: parsed
            .running
            .into_iter()
            .map(|(k, m)| {
                (
                    k,
                    RunningMeta {
                        from_agent: m.from_agent,
                        trace_id: m.trace_id,
                        started_at_ms: m.started_at_ms,
                        retries: m.retries,
                    },
                )
            })
            .collect(),
        rate_stamps: VecDeque::new(),
    }
}

fn append_dead_letter(
    state_dir: &Path,
    to_agent: &str,
    entry: &Entry,
    reason: &str,
) {
    let line = serde_json::json!({
        "ts_ms": unix_now_ms(),
        "to_agent": to_agent,
        "from_agent": entry.from_agent,
        "trace_id": entry.trace_id,
        "message": entry.message,
        "retries": entry.retries,
        "reason": reason,
    })
    .to_string();
    let _ = fs::create_dir_all(state_dir);
    use std::io::Write;
    if let Ok(mut f) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dead_file(state_dir))
    {
        let _ = writeln!(f, "{line}");
    }
}

// ─── Fila ────────────────────────────────────────────────────────────

pub struct AgentQueue {
    cfg: AgentQueueConfig,
    state: Mutex<State>,
}

impl AgentQueue {
    /// Cria a fila. `Arc<Self>` é requerido porque `enqueue` spawna
    /// tasks que precisam capturar `self`. Se `cfg.state_dir` é Some,
    /// restaura state.json (pending + running). Running restaurado
    /// significa "alguém disse 'tô rodando' mas eu não estou de fato"
    /// — reconcile() vai detectar (TTL) e ressuscitar.
    pub fn new(cfg: AgentQueueConfig) -> Arc<Self> {
        assert!(cfg.rate_max > 0, "rate_max deve ser > 0");
        let initial_state = match &cfg.state_dir {
            Some(dir) => restore_state(dir),
            None => State::default(),
        };
        Arc::new(Self {
            cfg,
            state: Mutex::new(initial_state),
        })
    }

    /// Persiste o state pro disco se `state_dir` está configurado.
    /// Chamado depois de cada mutação relevante. Assume que o caller
    /// soltou o lock — pega ele aqui.
    fn maybe_persist(&self) {
        if let Some(dir) = &self.cfg.state_dir {
            let st = self.state.lock().unwrap();
            persist_state(dir, &st);
        }
    }

    /// Enfileira um turn com prioridade Normal. Wrapper sobre
    /// [`Self::enqueue_with`] pra preservar a API antiga.
    pub fn enqueue(
        self: &Arc<Self>,
        to_agent: &str,
        from_agent: &str,
        message: &str,
        trace_id: Option<&str>,
    ) -> bool {
        self.enqueue_with(to_agent, from_agent, message, trace_id, Priority::Normal)
    }

    /// Enfileira com prioridade explícita. Insere mantendo a fila
    /// ordenada por prioridade (estável dentro do mesmo nível —
    /// posicionada no fim do bloco da prioridade dela).
    pub fn enqueue_with(
        self: &Arc<Self>,
        to_agent: &str,
        from_agent: &str,
        message: &str,
        trace_id: Option<&str>,
        priority: Priority,
    ) -> bool {
        if to_agent.is_empty() || from_agent.is_empty() {
            return false;
        }
        if !self.rate_ok() {
            if let Some(h) = &self.cfg.on_throttle {
                (h)(ThrottleEvent {
                    to_agent: to_agent.to_string(),
                    from_agent: from_agent.to_string(),
                    trace_id: trace_id.map(String::from),
                    reason: "rate_limit",
                    rate_max: self.cfg.rate_max,
                    rate_window_ms: self.cfg.rate_window.as_millis() as u64,
                });
            }
            return false;
        }

        let new_entry = Entry {
            from_agent: from_agent.to_string(),
            message: message.to_string(),
            trace_id: trace_id.map(String::from),
            enqueued_at_ms: unix_now_ms(),
            retries: 0,
            priority,
        };

        let size = {
            let mut st = self.state.lock().unwrap();
            let q = st.pending.entry(to_agent.to_string()).or_default();
            // Insere mantendo ordem: encontra primeiro item com
            // prioridade > new (= menos prioritário) e insere antes.
            // Estável dentro do mesmo nível (push_back se nenhum item
            // for menos prioritário).
            let insert_at = q
                .iter()
                .position(|e| e.priority > new_entry.priority)
                .unwrap_or(q.len());
            q.insert(insert_at, new_entry);
            q.len()
        };
        self.maybe_persist();

        if let Some(h) = &self.cfg.on_enqueue {
            (h)(EnqueueEvent {
                to_agent: to_agent.to_string(),
                from_agent: from_agent.to_string(),
                trace_id: trace_id.map(String::from),
                size,
            });
        }

        let this = Arc::clone(self);
        let to = to_agent.to_string();
        tokio::spawn(async move {
            this.drain_one(to).await;
        });
        true
    }

    /// Drena a fila de um agente. Se já está rodando, no-op. Se não há
    /// pending, no-op. Dentro do lock, tira o slot; fora do lock, chama
    /// runner. Re-entra se algo novo chegou durante o await.
    ///
    /// Pública pra permitir que callers disparem manualmente (ex.: tests
    /// determinísticos sem depender do `tokio::spawn`).
    // Assinatura explícita `impl Future + Send` é necessária: `drain_one`
    // é recursiva (re-spawna no final) e Rust não consegue inferir Send
    // do próprio corpo enquanto valida o corpo. Opt-out do clippy porque
    // `async fn` faria o build quebrar.
    #[allow(clippy::manual_async_fn)]
    pub fn drain_one(self: Arc<Self>, to_agent: String) -> impl Future<Output = ()> + Send {
        async move {
            if !self.try_acquire_slot(&to_agent) {
                return;
            }

            loop {
                let batch = self.take_batch(&to_agent);
                if batch.is_empty() {
                    break;
                }

                let from_label = if batch.len() == 1 {
                    batch[0].from_agent.clone()
                } else {
                    "múltiplos agentes".to_string()
                };
                let combined = if batch.len() == 1 {
                    batch[0].message.clone()
                } else {
                    batch
                        .iter()
                        .map(|e| format!("[{}]: {}", e.from_agent, e.message))
                        .collect::<Vec<_>>()
                        .join("\n\n")
                };
                let trace_ids: Vec<String> =
                    batch.iter().filter_map(|e| e.trace_id.clone()).collect();

                if let Some(h) = &self.cfg.on_drain_start {
                    (h)(DrainStartEvent {
                        to_agent: to_agent.clone(),
                        batch_size: batch.len(),
                        trace_ids: trace_ids.clone(),
                    });
                }

                let fut = (self.cfg.runner)(to_agent.clone(), from_label, combined, trace_ids);
                fut.await;
            }

            let has_more = self.release_slot(&to_agent);

            if let Some(h) = &self.cfg.on_drain_end {
                (h)(DrainEndEvent {
                    to_agent: to_agent.clone(),
                });
            }

            if has_more {
                let this = Arc::clone(&self);
                let to = to_agent;
                tokio::spawn(async move {
                    this.drain_one(to).await;
                });
            }
        }
    }

    // ─── helpers síncronos (isolam MutexGuard do frame async) ────────

    fn try_acquire_slot(&self, to_agent: &str) -> bool {
        let acquired = {
            let mut st = self.state.lock().unwrap();
            if st.running.contains_key(to_agent) {
                return false;
            }
            // Peek no front da pending pra puxar retries+from pro meta.
            let (from_agent, trace_id, retries) = match st.pending.get(to_agent).and_then(|q| q.front()) {
                Some(e) => (e.from_agent.clone(), e.trace_id.clone(), e.retries),
                None => return false,
            };
            st.running.insert(
                to_agent.to_string(),
                RunningMeta {
                    from_agent,
                    trace_id,
                    started_at_ms: unix_now_ms(),
                    retries,
                },
            );
            true
        };
        if acquired {
            self.maybe_persist();
        }
        acquired
    }

    fn take_batch(&self, to_agent: &str) -> Vec<Entry> {
        let batch = {
            let mut st = self.state.lock().unwrap();
            st.pending
                .get_mut(to_agent)
                .map(|q| q.drain(..).collect::<Vec<_>>())
                .unwrap_or_default()
        };
        if !batch.is_empty() {
            self.maybe_persist();
        }
        batch
    }

    fn release_slot(&self, to_agent: &str) -> bool {
        let has_more = {
            let mut st = self.state.lock().unwrap();
            st.running.remove(to_agent);
            st.pending
                .get(to_agent)
                .map(|q| !q.is_empty())
                .unwrap_or(false)
        };
        self.maybe_persist();
        has_more
    }

    /// Retorna true se o agente está atualmente processando um turn
    /// (foi adquirido via `acquire` ou pelo drain automático e ainda
    /// não soltou). Usado pelo endpoint de status pra que a UI saiba
    /// quando mostrar "pensando..." mesmo após reabrir o chat.
    pub fn is_processing(&self, to_agent: &str) -> bool {
        if to_agent.is_empty() {
            return false;
        }
        let st = self.state.lock().unwrap();
        st.running.contains_key(to_agent)
    }

    /// Reserva um agente bloqueando o drain automático. Usado pelo chat
    /// humano com streaming no `.cjs` (`acquire` antes de responder,
    /// `release` depois). No-op se `to_agent` vazio.
    pub fn acquire(&self, to_agent: &str) {
        if to_agent.is_empty() {
            return;
        }
        {
            let mut st = self.state.lock().unwrap();
            st.running.insert(
                to_agent.to_string(),
                RunningMeta {
                    from_agent: "human".to_string(),
                    trace_id: None,
                    started_at_ms: unix_now_ms(),
                    retries: 0,
                },
            );
        }
        self.maybe_persist();
    }

    /// Libera e re-dispara drain se houver pending. No-op se `to_agent`
    /// vazio.
    pub fn release(self: &Arc<Self>, to_agent: &str) {
        if to_agent.is_empty() {
            return;
        }
        let has_pending = {
            let mut st = self.state.lock().unwrap();
            st.running.remove(to_agent);
            st.pending
                .get(to_agent)
                .map(|q| !q.is_empty())
                .unwrap_or(false)
        };
        self.maybe_persist();
        if has_pending {
            let this = Arc::clone(self);
            let to = to_agent.to_string();
            tokio::spawn(async move {
                this.drain_one(to).await;
            });
        }
    }

    pub fn is_running(&self, to_agent: &str) -> bool {
        let st = self.state.lock().unwrap();
        st.running.contains_key(to_agent)
    }

    pub fn pending_size(&self, to_agent: &str) -> usize {
        let st = self.state.lock().unwrap();
        st.pending.get(to_agent).map(|q| q.len()).unwrap_or(0)
    }

    pub fn stats(&self) -> QueueStats {
        let st = self.state.lock().unwrap();
        let mut running: Vec<String> = st.running.keys().cloned().collect();
        running.sort();
        let pending: HashMap<String, usize> = st
            .pending
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, v)| (k.clone(), v.len()))
            .collect();
        QueueStats {
            running,
            pending,
            rate_used: st.rate_stamps.len(),
            rate_max: self.cfg.rate_max,
            rate_window_ms: self.cfg.rate_window.as_millis() as u64,
        }
    }

    /// Detecta turns "zumbis" — running há mais de `turn_ttl` (processo
    /// caiu, server reiniciou, etc.). Cada zumbi:
    ///   - se `retries + 1 < max_retries`: volta pro front de pending
    ///     com `retries + 1`. Vai re-rodar na próxima drain.
    ///   - se `retries + 1 >= max_retries`: vai pro `<state_dir>/dead.jsonl`
    ///     (se persistência ligada) e o hook on_dead_letter é chamado.
    ///
    /// Retorna `ReconcileResult` com contagens. Para chamar
    /// periodicamente, use spawn_reconciliation_loop.
    pub fn reconcile(&self) -> ReconcileResult {
        let now = unix_now_ms();
        let ttl_ms = self.cfg.turn_ttl.as_millis() as u64;
        let max_age_ms = self.cfg.message_max_age.as_millis() as u64;
        let max_retries = self.cfg.max_retries;
        let state_dir = self.cfg.state_dir.clone();

        let mut result = ReconcileResult::default();
        let mut dead_events: Vec<DeadLetterEvent> = Vec::new();
        let mut to_dead_letter: Vec<(String, Entry, &'static str)> = Vec::new();

        {
            let mut st = self.state.lock().unwrap();

            // (1) Expira mensagens velhas em pending — alguma esperou
            // mais que max_age sem chegar no runner. Vai direto pro
            // dead.jsonl com reason="expired".
            let agents: Vec<String> = st.pending.keys().cloned().collect();
            for agent in agents {
                let Some(q) = st.pending.get_mut(&agent) else { continue };
                let before = q.len();
                let mut kept: VecDeque<Entry> = VecDeque::with_capacity(before);
                while let Some(e) = q.pop_front() {
                    if now.saturating_sub(e.enqueued_at_ms) > max_age_ms {
                        dead_events.push(DeadLetterEvent {
                            to_agent: agent.clone(),
                            from_agent: e.from_agent.clone(),
                            trace_id: e.trace_id.clone(),
                            retries: e.retries,
                            reason: "expired",
                        });
                        to_dead_letter.push((agent.clone(), e, "expired"));
                        result.dead += 1;
                    } else {
                        kept.push_back(e);
                    }
                }
                *q = kept;
            }

            // (2) Zumbis: turns rodando há mais que TTL — processo morreu,
            // ressuscita pra pending ou dead-letter conforme retries.
            let zombies: Vec<String> = st
                .running
                .iter()
                .filter(|(_, m)| now.saturating_sub(m.started_at_ms) > ttl_ms)
                .map(|(k, _)| k.clone())
                .collect();
            for to_agent in zombies {
                let Some(meta) = st.running.remove(&to_agent) else {
                    continue;
                };
                let next_retries = meta.retries.saturating_add(1);
                let entry = Entry {
                    from_agent: meta.from_agent.clone(),
                    message: format!("[zumbi reciclado pela reconciliação] turn anterior ficou >{}s sem responder", ttl_ms / 1000),
                    trace_id: meta.trace_id.clone(),
                    enqueued_at_ms: now,
                    retries: next_retries,
                    // Reciclados ganham High pra processar antes dos outros
                    // (já esperaram além do TTL).
                    priority: Priority::High,
                };
                if next_retries >= max_retries {
                    to_dead_letter.push((to_agent.clone(), entry, "max_retries_zombie"));
                    dead_events.push(DeadLetterEvent {
                        to_agent,
                        from_agent: meta.from_agent,
                        trace_id: meta.trace_id,
                        retries: next_retries,
                        reason: "max_retries_zombie",
                    });
                    result.dead += 1;
                } else {
                    // Volta pra pending respeitando ordem por prioridade
                    // (zumbi é High → vai antes de Normal/Low já lá, mas
                    // depois de outras Highs pré-existentes).
                    let q = st.pending.entry(to_agent).or_default();
                    let insert_at = q
                        .iter()
                        .position(|e| e.priority > entry.priority)
                        .unwrap_or(q.len());
                    q.insert(insert_at, entry);
                    result.recycled += 1;
                }
            }
        }

        if let Some(dir) = state_dir.as_deref() {
            for (to_agent, entry, reason) in &to_dead_letter {
                append_dead_letter(dir, to_agent, entry, reason);
            }
        }
        if !to_dead_letter.is_empty() || result.recycled > 0 {
            self.maybe_persist();
        }

        if let Some(h) = &self.cfg.on_dead_letter {
            for ev in dead_events {
                (h)(ev);
            }
        }

        result
    }

    // ─── internos ────────────────────────────────────────────────────

    /// Expira stamps antigos e registra novo se ainda cabe. Lock é
    /// interno a esta função — não conflita com quem chama pending.
    fn rate_ok(&self) -> bool {
        let now = self.cfg.clock.now_millis();
        let window_ms = self.cfg.rate_window.as_millis() as u64;
        let mut st = self.state.lock().unwrap();
        while let Some(&front) = st.rate_stamps.front() {
            if now.saturating_sub(front) > window_ms {
                st.rate_stamps.pop_front();
            } else {
                break;
            }
        }
        if st.rate_stamps.len() >= self.cfg.rate_max {
            return false;
        }
        st.rate_stamps.push_back(now);
        true
    }
}
