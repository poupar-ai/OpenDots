//! Índice em memória `trace_id → eventos`.
//!
//! Hoje `get_message(mem_dir, trace_id)` lê e parsa TODOS os arquivos
//! dos últimos `retention_days` (default ~30) a cada chamada — O(N
//! eventos × R dias). Com índice em memória, vira O(1) lookup +
//! O(K eventos do trace) pra montar o summary.
//!
//! Uso típico (caller mantém um `Arc<TraceIndex>` no AppState):
//!   1. No startup: `index.build_from_disk(mem_dir, retention, clock)`.
//!   2. A cada `emit()`: `index.record(&event)`.
//!   3. Consultas: `index.get_trace(trace_id)` em vez de
//!      `get_message(mem_dir, trace_id, clock)`.
//!
//! A fonte de verdade continua sendo `messages/YYYY-MM-DD.jsonl` —
//! índice é cache derivado. Em crash, é só refazer build_from_disk
//! no próximo startup.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use serde_json::Value;

use crate::{read_events, trace_summary, Clock, ReadEventsOpts, TraceSummary};

#[derive(Default)]
pub struct TraceIndex {
    by_trace: Mutex<HashMap<String, Vec<Value>>>,
}

impl TraceIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Popula o índice lendo `read_events` com o mesmo retention que
    /// `get_message` usa por default. Idempotente — chamadas repetidas
    /// rebuild do zero. Pra reduzir startup time em dirs grandes, o
    /// caller pode pular essa chamada e construir lazy via `record`.
    pub fn build_from_disk(&self, mem_dir: &Path, retention_days: u32, clock: &dyn Clock) {
        let events = read_events(
            mem_dir,
            &ReadEventsOpts {
                since_days: Some(retention_days),
                since_date: None,
            },
            clock,
        );
        let mut map: HashMap<String, Vec<Value>> = HashMap::new();
        for ev in events {
            let Some(tid) = ev.get("trace_id").and_then(Value::as_str) else {
                continue;
            };
            map.entry(tid.to_string()).or_default().push(ev);
        }
        *self.by_trace.lock().unwrap() = map;
    }

    /// Registra um evento novo no índice. Chamar logo após `emit()`.
    /// Eventos sem `trace_id` são ignorados (sem chave pra indexar).
    pub fn record(&self, ev: &Value) {
        let Some(tid) = ev.get("trace_id").and_then(Value::as_str) else {
            return;
        };
        let mut map = self.by_trace.lock().unwrap();
        map.entry(tid.to_string()).or_default().push(ev.clone());
    }

    /// Consulta um trace. None se nenhum evento desse trace está no
    /// índice (pode ter caído em compactação ou ser fora da janela de
    /// retention que o build cobriu).
    pub fn get_trace(&self, trace_id: &str) -> Option<TraceSummary> {
        let map = self.by_trace.lock().unwrap();
        let events = map.get(trace_id)?;
        if events.is_empty() {
            return None;
        }
        trace_summary(events, trace_id)
    }

    /// Quantos traces estão indexados — telemetria útil pra health
    /// check / monitoring.
    pub fn len(&self) -> usize {
        self.by_trace.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_trace.lock().unwrap().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{emit, SystemClock};
    use serde_json::json;

    #[test]
    fn record_then_get_returns_trace_summary() {
        let idx = TraceIndex::new();
        idx.record(&json!({"type":"created","trace_id":"t-1","from":"a","to":"b"}));
        idx.record(&json!({"type":"delivered","trace_id":"t-1","to":"b","from":"a"}));
        idx.record(&json!({"type":"acked","trace_id":"t-1","by":"b"}));
        let summary = idx.get_trace("t-1").expect("trace deveria estar indexado");
        assert!(summary.created.is_some(), "created event esperado");
        assert_eq!(summary.events.len(), 3);
        // attempts conta delivered+retry (vide trace_summary).
        assert_eq!(summary.attempts, 1);
        assert!(idx.get_trace("t-nao-existe").is_none());
        assert_eq!(idx.len(), 1);
    }

    #[test]
    fn record_skips_events_without_trace_id() {
        let idx = TraceIndex::new();
        idx.record(&json!({"type":"noise","not_a_trace":"x"}));
        assert!(idx.is_empty());
    }

    #[test]
    fn build_from_disk_populates_from_events_jsonl() {
        let tmp = tempfile::TempDir::new().unwrap();
        let clock = SystemClock;
        // Emit eventos com diferentes trace_ids; emit grava no disco.
        emit(
            tmp.path(),
            &json!({"type":"created","trace_id":"trace-A","from":"x","to":"y"}),
            &clock,
        );
        emit(
            tmp.path(),
            &json!({"type":"created","trace_id":"trace-B","from":"x","to":"z"}),
            &clock,
        );
        emit(
            tmp.path(),
            &json!({"type":"delivered","trace_id":"trace-A","to":"y","from":"x"}),
            &clock,
        );

        let idx = TraceIndex::new();
        idx.build_from_disk(tmp.path(), 30, &clock);

        assert_eq!(idx.len(), 2, "esperava 2 traces indexados");
        let sa = idx.get_trace("trace-A").unwrap();
        assert_eq!(sa.events.len(), 2);
        let sb = idx.get_trace("trace-B").unwrap();
        assert_eq!(sb.events.len(), 1);
    }
}
