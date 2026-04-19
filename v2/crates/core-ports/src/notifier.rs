//! Port do bus de notificação interno.
//!
//! No `.cjs` original esse bus serve dois casos:
//! 1. **UI SSE** — eventos push para `/api/chat/stream` do servidor
//!    embutido. Ex.: nova mensagem de agente, mudança de status do
//!    cofre, heartbeat.
//! 2. **Agente → agente** — via `bus.cjs` + `router.cjs`, mensagens
//!    com deadlines (`DELIVERY_DEADLINE_MS=90s`,
//!    `RESPONSE_DEADLINE_MS=5min`) gravadas em
//!    `messages/YYYY-MM-DD.jsonl` e despachadas por sweep de 30s.
//!
//! O trait é intencionalmente minimalista: publicação e contagem de
//! subscribers. Quem resolve as duas necessidades é o adapter (SSE
//! broadcaster, ou router do bus de agentes).

use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum NotifierError {
    /// Nenhum subscriber no tópico. Implementações podem tratar como
    /// "ok, ninguém está ouvindo" (no-op) ou como erro — depende do
    /// caso. Por isso é erro, não `Result<(), ()>`: o caller decide.
    #[error("no subscribers")]
    NoSubscribers,
    /// Canal encerrado (shutdown graceful). Distinto de NoSubscribers
    /// porque é estado de bus, não de tópico.
    #[error("channel closed")]
    Closed,
}

pub trait Notifier: Send + Sync {
    /// Publica `event` em `topic`. `event` é `Value` para não forçar
    /// shape global — cada tópico define o seu.
    fn publish(&self, topic: &str, event: &Value) -> Result<(), NotifierError>;

    /// Quantidade atual de subscribers no tópico. Usado pela UI para
    /// "ninguém está assistindo, economizar trabalho de renderizar".
    fn subscriber_count(&self, topic: &str) -> usize;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    };

    #[derive(Default)]
    struct InMemNotifier {
        published: Mutex<Vec<(String, Value)>>,
        subs: AtomicUsize,
    }

    impl Notifier for InMemNotifier {
        fn publish(&self, topic: &str, event: &Value) -> Result<(), NotifierError> {
            self.published
                .lock()
                .unwrap()
                .push((topic.to_string(), event.clone()));
            Ok(())
        }
        fn subscriber_count(&self, _topic: &str) -> usize {
            self.subs.load(Ordering::SeqCst)
        }
    }

    #[test]
    fn publish_is_recorded() {
        let n = InMemNotifier::default();
        n.publish("chat", &serde_json::json!({"msg": "hi"})).unwrap();
        assert_eq!(n.published.lock().unwrap().len(), 1);
    }
}
