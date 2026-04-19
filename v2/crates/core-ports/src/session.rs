//! Port de estado de sessão.
//!
//! No `.cjs` original, o Claude Code roda um hook `session-start` que
//! precisa injetar contexto do projeto **uma vez só** por sessão.
//! A convenção para marcar "essa sessão já foi inicializada" é um
//! arquivo flag vazio em `<memDir>/tmp/.init-<session_id>`.
//!
//! Este port abstrai essa semântica. O adapter padrão (`adapter-session-file`)
//! implementa via `std::fs` tocando o flag file; implementações
//! alternativas (in-memory para testes, Redis para multi-host) podem
//! plugar sem mexer no hook.

use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("io error on {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

/// Gerencia o ciclo init → clear de uma sessão identificada por
/// `session_id` (string opaca vinda do Claude Code via stdin JSON).
pub trait SessionStore: Send + Sync {
    /// `true` se já existe marca de inicialização para esta sessão.
    fn is_initialized(&self, tmp_dir: &Path, session_id: &str) -> Result<bool, SessionError>;

    /// Marca sessão como inicializada. Idempotente — chamar duas vezes
    /// seguidas não deve gerar erro.
    fn mark_initialized(&self, tmp_dir: &Path, session_id: &str) -> Result<(), SessionError>;

    /// Remove a marca. Chamado pelo hook `session-end`. Idempotente:
    /// limpar o que não existe não é erro.
    fn clear(&self, tmp_dir: &Path, session_id: &str) -> Result<(), SessionError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::Mutex;

    #[derive(Default)]
    struct InMemory {
        set: Mutex<HashSet<String>>,
    }

    impl SessionStore for InMemory {
        fn is_initialized(&self, _tmp: &Path, s: &str) -> Result<bool, SessionError> {
            Ok(self.set.lock().unwrap().contains(s))
        }
        fn mark_initialized(&self, _tmp: &Path, s: &str) -> Result<(), SessionError> {
            self.set.lock().unwrap().insert(s.to_string());
            Ok(())
        }
        fn clear(&self, _tmp: &Path, s: &str) -> Result<(), SessionError> {
            self.set.lock().unwrap().remove(s);
            Ok(())
        }
    }

    #[test]
    fn lifecycle() {
        let store = InMemory::default();
        let p = Path::new("/tmp");
        assert!(!store.is_initialized(p, "s1").unwrap());
        store.mark_initialized(p, "s1").unwrap();
        assert!(store.is_initialized(p, "s1").unwrap());
        store.clear(p, "s1").unwrap();
        assert!(!store.is_initialized(p, "s1").unwrap());
    }
}
