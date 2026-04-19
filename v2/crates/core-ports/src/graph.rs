//! Port de persistência do grafo de projeto (`graph.json`).
//!
//! O grafo v2 mapeia 6 tipos de nó (file, command, agent, human,
//! decision, gotcha) conectados por 13 tipos de aresta, com pesos
//! temporais (coupling window 15min, half-life 30 dias). O port
//! não sabe disso — trabalha em cima de `serde_json::Value` opaco
//! para preservar paridade byte-a-byte com o `.cjs`.
//!
//! O domínio (`domain-graph`) define structs tipados, manipula, e
//! serializa de volta para `Value` antes de chamar `save`.

use serde_json::Value;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    #[error("io on {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
}

pub trait GraphStore: Send + Sync {
    /// Retorna `None` se `graph.json` não existe. Caller decide se
    /// rebuilda a partir de observações ou trata como grafo vazio.
    fn load(&self, mem_dir: &Path) -> Result<Option<Value>, GraphError>;

    /// Grava `graph.json`. Adapter deve usar indent de 2 spaces — o
    /// `.cjs` faz `JSON.stringify(graph, null, 2)`. Mudar o indent
    /// quebra paridade de diff de arquivos.
    fn save(&self, mem_dir: &Path, graph: &Value) -> Result<(), GraphError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct InMemGraph {
        current: Mutex<Option<Value>>,
    }

    impl GraphStore for InMemGraph {
        fn load(&self, _mem: &Path) -> Result<Option<Value>, GraphError> {
            Ok(self.current.lock().unwrap().clone())
        }
        fn save(&self, _mem: &Path, graph: &Value) -> Result<(), GraphError> {
            *self.current.lock().unwrap() = Some(graph.clone());
            Ok(())
        }
    }

    #[test]
    fn roundtrip() {
        let store = InMemGraph::default();
        let p = Path::new("/tmp");
        assert!(store.load(p).unwrap().is_none());
        let g = serde_json::json!({ "version": 2, "nodes": {}, "edges": {} });
        store.save(p, &g).unwrap();
        assert_eq!(store.load(p).unwrap().unwrap(), g);
    }
}
