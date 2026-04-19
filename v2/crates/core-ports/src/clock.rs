//! Port de tempo.
//!
//! Isola `Utc::now()` para permitir `FakeClock` nos testes de domínio
//! que dependem de janelas de tempo (ex.: coupling window de 15min no
//! grafo, deadlines do bus de agentes, TTL do cofre).
//!
//! Sem este port, testar "X expira depois de 90s" exige sleep real ou
//! hack em `mockall`. Com ele, o domínio injeta um clock controlável.

use chrono::{DateTime, Utc};

/// Fonte de tempo injetável. `Send + Sync` porque é compartilhado
/// entre threads (ex.: sweep do bus roda em task própria).
pub trait Clock: Send + Sync {
    /// Instante atual em UTC. Em produção delega para `Utc::now()`;
    /// em testes retorna valor controlado.
    fn now(&self) -> DateTime<Utc>;
}

/// Clock de produção — thin wrapper em `Utc::now()`.
///
/// `Copy` + `Default` para plugar sem ceremônia:
/// `Arc::new(SystemClock) as Arc<dyn Clock>`.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicI64, Ordering};

    /// Fake de exemplo — valida que o trait é implementável com estado
    /// mutável controlado. Domínios reais vão ter o seu próprio.
    struct FakeClock(AtomicI64);

    impl Clock for FakeClock {
        fn now(&self) -> DateTime<Utc> {
            let secs = self.0.load(Ordering::SeqCst);
            DateTime::<Utc>::from_timestamp(secs, 0).unwrap()
        }
    }

    #[test]
    fn system_clock_returns_monotonically() {
        let c = SystemClock;
        let a = c.now();
        let b = c.now();
        assert!(b >= a);
    }

    #[test]
    fn fake_clock_is_controllable() {
        let c = FakeClock(AtomicI64::new(1_700_000_000));
        assert_eq!(c.now().timestamp(), 1_700_000_000);
    }
}
