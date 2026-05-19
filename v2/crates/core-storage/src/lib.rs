//! Repository pattern sobre arquivos JSONL append-only.
//!
//! Centraliza acesso ao "banco" do plugin (memória persistente por
//! projeto). Substitui o pattern atual de cada handler abrir o arquivo
//! com `fs::OpenOptions` e parsear linha a linha manualmente —
//! repetitivo, sem schema enforcement, fácil de errar.
//!
//! ## Garantias
//!
//! - **Schema enforcement**: cada coleção tem um `T: Serialize +
//!   DeserializeOwned`. Items malformados na leitura são logados como
//!   warning mas não derrubam o caller (mesma filosofia do .cjs).
//! - **Rotação automática**: arquivos > `max_bytes` ou com > `max_lines`
//!   são rotacionados pra `<name>.<YYYYMMDDHHMMSS>.jsonl`. Reads
//!   transparentes (`read_all` concatena partes).
//! - **Append O(1)** com fsync opcional. Throughput esperado: ~10k
//!   linhas/segundo em SSD com fsync off, ~1k/s com fsync on.
//!
//! ## Pra alta carga
//!
//! Hoje cada `append` faz 1 syscall write + (opcional) fsync. Pra
//! cenários de milhares/min, isso ainda é OK. Se virar gargalo,
//! adicionar buffered writer com flush periódico em background —
//! mas isso troca durabilidade por throughput, então deixo opt-in
//! quando vier a precisar.

use std::fs;
use std::io::Write;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialize: {0}")]
    Serialize(serde_json::Error),
}

/// Política de rotação. Quando QUALQUER um dos limites é atingido,
/// arquivo é rotacionado. Defaults pensados pra carga moderada (~1000
/// linhas/min):
///
/// - `max_lines = 50_000` → ~30 min de buffer em carga alta
/// - `max_bytes = 10 MB` → cap pra reads não explodir memory
/// - `fsync_on_append = false` → throughput > durabilidade (crash perde
///   só o último flush; aceitável pra logs append-only)
#[derive(Debug, Clone)]
pub struct RotationPolicy {
    pub max_lines: usize,
    pub max_bytes: u64,
    pub fsync_on_append: bool,
}

impl Default for RotationPolicy {
    fn default() -> Self {
        Self {
            max_lines: 50_000,
            max_bytes: 10 * 1024 * 1024,
            fsync_on_append: false,
        }
    }
}

/// Repository sobre um único arquivo JSONL (com rotação). Thread-safe
/// via Mutex interno — múltiplos threads podem appendar concorrente.
///
/// Generic em `T: Serialize + DeserializeOwned` — o tipo de cada
/// linha. Items malformados na leitura são silenciosamente ignorados
/// (logged a stderr).
pub struct JsonlRepository<T> {
    path: PathBuf,
    policy: RotationPolicy,
    line_count: Mutex<usize>,
    _marker: PhantomData<T>,
}

impl<T: Serialize + DeserializeOwned> JsonlRepository<T> {
    /// Abre o repositório no `path`. Lê o arquivo atual pra contar
    /// linhas (necessário pra checar `max_lines` no append). Se arquivo
    /// não existe, conta 0.
    pub fn open(path: PathBuf, policy: RotationPolicy) -> Result<Self, StorageError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let line_count = match fs::read_to_string(&path) {
            Ok(content) => content.lines().filter(|l| !l.is_empty()).count(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
            Err(e) => return Err(StorageError::Io(e)),
        };
        Ok(Self {
            path,
            policy,
            line_count: Mutex::new(line_count),
            _marker: PhantomData,
        })
    }

    pub fn open_default(path: PathBuf) -> Result<Self, StorageError> {
        Self::open(path, RotationPolicy::default())
    }

    /// Append 1 item. Serializa, escreve linha, atualiza count, rotaciona
    /// se passou do limite. Hot path da escrita — mantido o mais magro
    /// possível.
    pub fn append(&self, item: &T) -> Result<(), StorageError> {
        let line = serde_json::to_string(item).map_err(StorageError::Serialize)?;
        let mut count = self.line_count.lock().unwrap();
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(f, "{line}")?;
        if self.policy.fsync_on_append {
            f.sync_data()?;
        }
        *count += 1;
        // Rotaciona se passou do limite de linhas OU bytes.
        if *count >= self.policy.max_lines
            || fs::metadata(&self.path)
                .map(|m| m.len() >= self.policy.max_bytes)
                .unwrap_or(false)
        {
            drop(f);
            self.rotate_inner(&mut count)?;
        }
        Ok(())
    }

    /// Append N items em UMA chamada — uma única abertura+fsync no
    /// fim. ~10× mais rápido que loop de `append` em alta carga.
    pub fn append_batch(&self, items: &[T]) -> Result<(), StorageError> {
        if items.is_empty() {
            return Ok(());
        }
        let mut buf = String::with_capacity(items.len() * 256);
        for item in items {
            let line = serde_json::to_string(item).map_err(StorageError::Serialize)?;
            buf.push_str(&line);
            buf.push('\n');
        }
        let mut count = self.line_count.lock().unwrap();
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        f.write_all(buf.as_bytes())?;
        if self.policy.fsync_on_append {
            f.sync_data()?;
        }
        *count += items.len();
        if *count >= self.policy.max_lines
            || fs::metadata(&self.path)
                .map(|m| m.len() >= self.policy.max_bytes)
                .unwrap_or(false)
        {
            drop(f);
            self.rotate_inner(&mut count)?;
        }
        Ok(())
    }

    /// Lê TODAS as linhas — do arquivo atual + qualquer arquivo
    /// rotacionado. Linhas malformadas são puladas com warning.
    pub fn read_all(&self) -> Vec<T> {
        let mut out = Vec::new();
        for part in self.all_parts() {
            let Ok(content) = fs::read_to_string(&part) else { continue };
            for line in content.lines() {
                if line.is_empty() {
                    continue;
                }
                match serde_json::from_str::<T>(line) {
                    Ok(v) => out.push(v),
                    Err(e) => {
                        eprintln!("jsonl warning ({}): linha malformada: {e}", part.display());
                    }
                }
            }
        }
        out
    }

    /// Lê só as últimas `n` linhas do arquivo ATUAL (sem partes
    /// rotacionadas — comum em tail/history). Usa leitura inteira do
    /// arquivo + take(n) do fim. Pra carga muito alta, otimizar com
    /// scan de trás pra frente.
    pub fn tail(&self, n: usize) -> Vec<T> {
        let Ok(content) = fs::read_to_string(&self.path) else { return Vec::new() };
        let lines: Vec<&str> = content.lines().filter(|l| !l.is_empty()).collect();
        let start = lines.len().saturating_sub(n);
        lines[start..]
            .iter()
            .filter_map(|line| serde_json::from_str::<T>(line).ok())
            .collect()
    }

    /// Quantas linhas no arquivo atual (sem partes rotacionadas).
    pub fn current_line_count(&self) -> usize {
        *self.line_count.lock().unwrap()
    }

    /// Lista paths de todas as partes: rotacionadas + atual. Em ordem
    /// cronológica (mais antigas primeiro).
    pub fn all_parts(&self) -> Vec<PathBuf> {
        let Some(parent) = self.path.parent() else { return vec![self.path.clone()] };
        let Some(stem) = self.path.file_stem().and_then(|s| s.to_str()) else {
            return vec![self.path.clone()];
        };
        let mut parts: Vec<PathBuf> = match fs::read_dir(parent) {
            Ok(entries) => entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    // formato: <stem>.<timestamp>.jsonl ou <stem>.jsonl
                    name.starts_with(&format!("{stem}."))
                        && name.ends_with(".jsonl")
                        && p != &self.path
                })
                .collect(),
            Err(_) => Vec::new(),
        };
        parts.sort();
        parts.push(self.path.clone());
        parts
    }

    /// Força rotação. Útil pra testes ou pra rotacionar antes de
    /// snapshot/export.
    pub fn rotate(&self) -> Result<Option<PathBuf>, StorageError> {
        let mut count = self.line_count.lock().unwrap();
        self.rotate_inner(&mut count)
    }

    fn rotate_inner(
        &self,
        count: &mut usize,
    ) -> Result<Option<PathBuf>, StorageError> {
        if !self.path.exists() || *count == 0 {
            return Ok(None);
        }
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let Some(parent) = self.path.parent() else { return Ok(None) };
        let Some(stem) = self.path.file_stem().and_then(|s| s.to_str()) else { return Ok(None) };
        let rotated = parent.join(format!("{stem}.{ts}.jsonl"));
        fs::rename(&self.path, &rotated)?;
        *count = 0;
        Ok(Some(rotated))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
    struct Sample {
        id: u32,
        msg: String,
    }

    fn repo(tmp: &tempfile::TempDir, name: &str) -> JsonlRepository<Sample> {
        JsonlRepository::open_default(tmp.path().join(format!("{name}.jsonl"))).unwrap()
    }

    #[test]
    fn append_then_read_roundtrip() {
        let tmp = tempfile::TempDir::new().unwrap();
        let r = repo(&tmp, "x");
        r.append(&Sample { id: 1, msg: "a".into() }).unwrap();
        r.append(&Sample { id: 2, msg: "b".into() }).unwrap();
        r.append(&Sample { id: 3, msg: "c".into() }).unwrap();
        let all = r.read_all();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].id, 1);
        assert_eq!(all[2].msg, "c");
    }

    #[test]
    fn tail_returns_last_n() {
        let tmp = tempfile::TempDir::new().unwrap();
        let r = repo(&tmp, "x");
        for i in 0..10 {
            r.append(&Sample { id: i, msg: format!("m{i}") }).unwrap();
        }
        let t = r.tail(3);
        assert_eq!(t.len(), 3);
        assert_eq!(t[0].id, 7);
        assert_eq!(t[2].id, 9);
    }

    #[test]
    fn malformed_line_is_skipped_not_panic() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("x.jsonl");
        fs::write(&path, "{\"id\":1,\"msg\":\"ok\"}\n{ broken json\n{\"id\":2,\"msg\":\"ok2\"}\n").unwrap();
        let r: JsonlRepository<Sample> =
            JsonlRepository::open_default(path).unwrap();
        let all = r.read_all();
        assert_eq!(all.len(), 2, "linha malformada deveria ter sido pulada");
        assert_eq!(all[0].id, 1);
        assert_eq!(all[1].id, 2);
    }

    #[test]
    fn rotation_by_line_count_triggers() {
        let tmp = tempfile::TempDir::new().unwrap();
        let policy = RotationPolicy {
            max_lines: 5,
            max_bytes: u64::MAX,
            fsync_on_append: false,
        };
        let r: JsonlRepository<Sample> =
            JsonlRepository::open(tmp.path().join("x.jsonl"), policy).unwrap();
        for i in 0..5 {
            r.append(&Sample { id: i, msg: "m".into() }).unwrap();
        }
        // 5ª append atinge max_lines → rotaciona após. Próximo append
        // vai pro arquivo novo.
        assert_eq!(r.current_line_count(), 0, "rotacionou");
        r.append(&Sample { id: 100, msg: "novo".into() }).unwrap();
        assert_eq!(r.current_line_count(), 1);
        // read_all deve concatenar rotacionado + atual.
        let all = r.read_all();
        assert_eq!(all.len(), 6);
        assert_eq!(all[0].id, 0);
        assert_eq!(all[5].id, 100);
    }

    #[test]
    fn append_batch_writes_all_in_one_call() {
        let tmp = tempfile::TempDir::new().unwrap();
        let r = repo(&tmp, "x");
        let batch: Vec<Sample> = (0..1000)
            .map(|i| Sample { id: i, msg: format!("m{i}") })
            .collect();
        r.append_batch(&batch).unwrap();
        assert_eq!(r.current_line_count(), 1000);
        let all = r.read_all();
        assert_eq!(all.len(), 1000);
        assert_eq!(all[0].id, 0);
        assert_eq!(all[999].id, 999);
    }

    #[test]
    fn rotate_explicit_creates_new_part() {
        let tmp = tempfile::TempDir::new().unwrap();
        let r = repo(&tmp, "x");
        r.append(&Sample { id: 1, msg: "a".into() }).unwrap();
        let rotated = r.rotate().unwrap();
        assert!(rotated.is_some());
        assert_eq!(r.current_line_count(), 0);
        r.append(&Sample { id: 2, msg: "b".into() }).unwrap();
        let parts = r.all_parts();
        assert_eq!(parts.len(), 2, "rotacionada + atual");
    }
}
