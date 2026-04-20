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

use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
}

impl AgentQueueConfig {
    /// Cria config com defaults do `.cjs` (rate_max=40, 60s, sem hooks,
    /// SystemClock).
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
        }
    }
}

// ─── Estado interno ──────────────────────────────────────────────────

struct Entry {
    from_agent: String,
    message: String,
    trace_id: Option<String>,
}

#[derive(Default)]
struct State {
    running: HashSet<String>,
    pending: HashMap<String, VecDeque<Entry>>,
    rate_stamps: VecDeque<u64>,
}

// ─── Fila ────────────────────────────────────────────────────────────

pub struct AgentQueue {
    cfg: AgentQueueConfig,
    state: Mutex<State>,
}

impl AgentQueue {
    /// Cria a fila. `Arc<Self>` é requerido porque `enqueue` spawna
    /// tasks que precisam capturar `self`.
    pub fn new(cfg: AgentQueueConfig) -> Arc<Self> {
        assert!(cfg.rate_max > 0, "rate_max deve ser > 0");
        Arc::new(Self {
            cfg,
            state: Mutex::new(State::default()),
        })
    }

    /// Enfileira um turn. Retorna `false` se args vazios ou rate-limit
    /// atingido. Spawn do drain acontece via `tokio::spawn` (equivalente
    /// a `setImmediate` do `.cjs` — mesmo contrato de "processa depois").
    pub fn enqueue(
        self: &Arc<Self>,
        to_agent: &str,
        from_agent: &str,
        message: &str,
        trace_id: Option<&str>,
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

        let size = {
            let mut st = self.state.lock().unwrap();
            let q = st.pending.entry(to_agent.to_string()).or_default();
            q.push_back(Entry {
                from_agent: from_agent.to_string(),
                message: message.to_string(),
                trace_id: trace_id.map(String::from),
            });
            q.len()
        };

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
        let mut st = self.state.lock().unwrap();
        if st.running.contains(to_agent) {
            return false;
        }
        let empty = st
            .pending
            .get(to_agent)
            .map(|q| q.is_empty())
            .unwrap_or(true);
        if empty {
            return false;
        }
        st.running.insert(to_agent.to_string());
        true
    }

    fn take_batch(&self, to_agent: &str) -> Vec<Entry> {
        let mut st = self.state.lock().unwrap();
        st.pending
            .get_mut(to_agent)
            .map(|q| q.drain(..).collect())
            .unwrap_or_default()
    }

    fn release_slot(&self, to_agent: &str) -> bool {
        let mut st = self.state.lock().unwrap();
        st.running.remove(to_agent);
        st.pending
            .get(to_agent)
            .map(|q| !q.is_empty())
            .unwrap_or(false)
    }

    /// Reserva um agente bloqueando o drain automático. Usado pelo chat
    /// humano com streaming no `.cjs` (`acquire` antes de responder,
    /// `release` depois). No-op se `to_agent` vazio.
    pub fn acquire(&self, to_agent: &str) {
        if to_agent.is_empty() {
            return;
        }
        let mut st = self.state.lock().unwrap();
        st.running.insert(to_agent.to_string());
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
        st.running.contains(to_agent)
    }

    pub fn pending_size(&self, to_agent: &str) -> usize {
        let st = self.state.lock().unwrap();
        st.pending.get(to_agent).map(|q| q.len()).unwrap_or(0)
    }

    pub fn stats(&self) -> QueueStats {
        let st = self.state.lock().unwrap();
        let mut running: Vec<String> = st.running.iter().cloned().collect();
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
