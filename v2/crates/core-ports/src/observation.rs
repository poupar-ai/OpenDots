//! Port de log append-only em JSONL.
//!
//! Usado por `observations.jsonl`, `sessions.jsonl`, `agent_chats/*.jsonl`,
//! `human_logs/*.jsonl`, `messages/YYYY-MM-DD.jsonl` e mais. Todo arquivo
//! de memória do open-poupar que é **sequência imutável de eventos** passa
//! por este port.
//!
//! ## Por que o payload é `serde_json::Value`
//!
//! Cada domínio tem seu próprio shape (uma `Observation` não é uma
//! `AgentMessage`). Pôr o tipo concreto no trait forçaria um enum
//! gigante compartilhado ou um monte de traits quase-iguais. `Value`
//! mantém o port ignorante do conteúdo e deixa o domínio definir seus
//! structs + serializar antes de chamar `append`.
//!
//! Adapter é quem sabe:
//! - Abrir em append mode com `O_APPEND` (write atômico em linhas curtas).
//! - Serializar `Value` + `\n`.
//! - Ler backward em chunks para `read_last` sem ler o arquivo todo.

use serde_json::Value;
use std::path::Path;

/// Erros possíveis de leitura/escrita. `Io` carrega o path que falhou
/// porque debug de "permission denied" sem path é inútil.
#[derive(Debug, thiserror::Error)]
pub enum ObservationError {
    #[error("io error on {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
}

/// Contrato mínimo de um log JSONL:
///
/// - **append** uma linha (sem newline intermediário — adapter cuida).
/// - **read_all** — lê tudo; cuidado com arquivos grandes, usar só em
///   caminhos que sabidamente têm pouco volume (ex.: sessions.jsonl).
/// - **read_last** — tail único; adapter deve implementar com leitura
///   backward, não reler o arquivo inteiro.
pub trait ObservationLog: Send + Sync {
    /// Serializa `entry` como JSON e anexa uma linha ao arquivo.
    /// Cria o arquivo se não existir.
    fn append(&self, path: &Path, entry: &Value) -> Result<(), ObservationError>;

    /// Lê todas as entradas. Retorna `Ok(vec![])` se o arquivo não
    /// existe (paridade com `readJsonl` do `.cjs`).
    fn read_all(&self, path: &Path) -> Result<Vec<Value>, ObservationError>;

    /// Última entrada ou `Ok(None)` se o arquivo não existe / está
    /// vazio. Adapter deve usar leitura backward em chunks.
    fn read_last(&self, path: &Path) -> Result<Option<Value>, ObservationError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// In-memory para validar contrato sem tocar disco. Adapters reais
    /// vivem em `adapter-observations-file`.
    #[derive(Default)]
    struct InMemoryLog {
        entries: Mutex<Vec<Value>>,
    }

    impl ObservationLog for InMemoryLog {
        fn append(&self, _path: &Path, entry: &Value) -> Result<(), ObservationError> {
            self.entries.lock().unwrap().push(entry.clone());
            Ok(())
        }
        fn read_all(&self, _path: &Path) -> Result<Vec<Value>, ObservationError> {
            Ok(self.entries.lock().unwrap().clone())
        }
        fn read_last(&self, _path: &Path) -> Result<Option<Value>, ObservationError> {
            Ok(self.entries.lock().unwrap().last().cloned())
        }
    }

    #[test]
    fn append_read_roundtrip() {
        let log = InMemoryLog::default();
        let p = Path::new("obs.jsonl");
        log.append(p, &serde_json::json!({ "type": "file_edit", "file": "a.rs" }))
            .unwrap();
        log.append(p, &serde_json::json!({ "type": "bash", "cmd": "cargo test" }))
            .unwrap();
        assert_eq!(log.read_all(p).unwrap().len(), 2);
        let last = log.read_last(p).unwrap().unwrap();
        assert_eq!(last["type"], "bash");
    }
}
