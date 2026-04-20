//! Filesystem-backed implementations of [`core_ports::ObservationLog`],
//! [`core_ports::GraphStore`] and [`core_ports::SessionStore`].
//!
//! Todos os três adapters replicam a semântica do `plugin/scripts/lib.cjs`
//! e de `graph.cjs` / `session-init.cjs` / `session-end.cjs` byte-a-byte.
//!
//! - `ObservationLogFs` — JSONL append-only (`appendJsonl` / `readJsonl` /
//!   `readLastJsonlLine`). `append` anexa `JSON.stringify(obj) + '\n'`;
//!   `read_all` tolera linhas mal-formadas (pula — mesmo `catch { return null }`
//!   do .cjs); `read_last` faz leitura backward em chunks de 2048 bytes até
//!   achar uma linha válida.
//! - `GraphStoreFs` — `graph.json` com indent de 2 spaces e ordem de chaves
//!   preservada (usa `serde_json` com feature `preserve_order`). `load` retorna
//!   `None` se o arquivo não existe; `save` escreve os bytes crus que o
//!   caller passar (update de `updated_at` é responsabilidade do domain).
//! - `SessionStoreFs` — flag file vazio em `<tmp_dir>/.init-<session_id>`.
//!   `mark_initialized` é idempotente (truncate), `clear` engole erros de
//!   remove.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use core_ports::{
    GraphError, GraphStore, ObservationError, ObservationLog, SessionError, SessionStore,
};
use serde_json::Value;

// ─── Constantes ──────────────────────────────────────────────────────

/// Tamanho do chunk de leitura backward em `read_last` — mesmo valor do
/// `CHUNK = 2048` em `readLastJsonlLine` do `.cjs`. Manter igual para
/// preservar o ponto em que o scan para quando há linhas parcialmente
/// válidas.
pub const TAIL_CHUNK: usize = 2048;

/// Nome do arquivo do grafo relativo a `mem_dir`. Espelha
/// `GRAPH_FILE = 'graph.json'` em `graph.cjs`.
pub const GRAPH_FILE: &str = "graph.json";

// ─── ObservationLogFs ────────────────────────────────────────────────

/// Implementação stateless: todo o estado vive no caminho passado em
/// cada chamada. Derivar `Default` mantém o crate drop-in em wiring.
#[derive(Default, Clone, Copy, Debug)]
pub struct ObservationLogFs;

impl ObservationLogFs {
    pub fn new() -> Self {
        Self
    }
}

impl ObservationLog for ObservationLogFs {
    fn append(&self, path: &Path, entry: &Value) -> Result<(), ObservationError> {
        let mut line = serde_json::to_string(entry)?;
        line.push('\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|source| ObservationError::Io {
                path: path.display().to_string(),
                source,
            })?;
        file.write_all(line.as_bytes())
            .map_err(|source| ObservationError::Io {
                path: path.display().to_string(),
                source,
            })
    }

    fn read_all(&self, path: &Path) -> Result<Vec<Value>, ObservationError> {
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => {
                return Err(ObservationError::Io {
                    path: path.display().to_string(),
                    source,
                });
            }
        };
        let text = String::from_utf8_lossy(&bytes);
        let mut out = Vec::new();
        for line in text.split('\n') {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<Value>(line) {
                out.push(v);
            }
            // Linhas mal-formadas são silenciosamente puladas — paridade
            // com `try { JSON.parse } catch { null }` em `readJsonl`.
        }
        Ok(out)
    }

    fn read_last(&self, path: &Path) -> Result<Option<Value>, ObservationError> {
        let meta = match fs::metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(ObservationError::Io {
                    path: path.display().to_string(),
                    source,
                });
            }
        };
        let size = meta.len();
        if size == 0 {
            return Ok(None);
        }

        let mut file = File::open(path).map_err(|source| ObservationError::Io {
            path: path.display().to_string(),
            source,
        })?;

        let mut pos = size;
        let mut buf = String::new();

        while pos > 0 {
            let read_size = std::cmp::min(TAIL_CHUNK as u64, pos);
            pos -= read_size;
            file.seek(SeekFrom::Start(pos))
                .map_err(|source| ObservationError::Io {
                    path: path.display().to_string(),
                    source,
                })?;
            let mut chunk = vec![0u8; read_size as usize];
            file.read_exact(&mut chunk)
                .map_err(|source| ObservationError::Io {
                    path: path.display().to_string(),
                    source,
                })?;
            let mut head = String::from_utf8_lossy(&chunk).into_owned();
            head.push_str(&buf);
            buf = head;

            if let Some(last) = buf.split('\n').filter(|l| !l.trim().is_empty()).next_back() {
                if let Ok(v) = serde_json::from_str::<Value>(last) {
                    return Ok(Some(v));
                }
                // Linha mal-formada na ponta: continua scan para trás —
                // mesmo comportamento do `catch { /* keep scanning */ }`.
            }
        }
        Ok(None)
    }
}

// ─── GraphStoreFs ────────────────────────────────────────────────────

#[derive(Default, Clone, Copy, Debug)]
pub struct GraphStoreFs;

impl GraphStoreFs {
    pub fn new() -> Self {
        Self
    }
}

fn graph_path(mem_dir: &Path) -> PathBuf {
    mem_dir.join(GRAPH_FILE)
}

impl GraphStore for GraphStoreFs {
    fn load(&self, mem_dir: &Path) -> Result<Option<Value>, GraphError> {
        let path = graph_path(mem_dir);
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(GraphError::Io {
                    path: path.display().to_string(),
                    source,
                });
            }
        };
        let value: Value = serde_json::from_slice(&bytes)?;
        Ok(Some(value))
    }

    fn save(&self, mem_dir: &Path, graph: &Value) -> Result<(), GraphError> {
        let text = serde_json::to_string_pretty(graph)?;
        let path = graph_path(mem_dir);
        fs::write(&path, text).map_err(|source| GraphError::Io {
            path: path.display().to_string(),
            source,
        })
    }
}

// ─── SessionStoreFs ──────────────────────────────────────────────────

#[derive(Default, Clone, Copy, Debug)]
pub struct SessionStoreFs;

impl SessionStoreFs {
    pub fn new() -> Self {
        Self
    }
}

fn flag_path(tmp_dir: &Path, session_id: &str) -> PathBuf {
    tmp_dir.join(format!(".init-{session_id}"))
}

impl SessionStore for SessionStoreFs {
    fn is_initialized(&self, tmp_dir: &Path, session_id: &str) -> Result<bool, SessionError> {
        Ok(flag_path(tmp_dir, session_id).exists())
    }

    fn mark_initialized(&self, tmp_dir: &Path, session_id: &str) -> Result<(), SessionError> {
        let path = flag_path(tmp_dir, session_id);
        // Truncate mode espelha `fs.writeFileSync(flagFile, '')` do .cjs —
        // chamar duas vezes não acumula nem falha.
        File::create(&path)
            .map(|_| ())
            .map_err(|source| SessionError::Io {
                path: path.display().to_string(),
                source,
            })
    }

    fn clear(&self, tmp_dir: &Path, session_id: &str) -> Result<(), SessionError> {
        let path = flag_path(tmp_dir, session_id);
        // Idempotente: `NotFound` não é erro (paridade com o try/catch do
        // session-end.cjs). Outros erros propagam.
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(SessionError::Io {
                path: path.display().to_string(),
                source,
            }),
        }
    }
}
