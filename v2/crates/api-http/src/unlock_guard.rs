//! Rate-limit + persistência pra tentativas de `/api/vault/unlock`.
//!
//! Paridade com `server.cjs:471-541`:
//! - Janela de 60s, máximo 5 falhas antes de lockout de 60s.
//! - Estado persistido em `<memDir>/.vault-unlock-state.json` pra
//!   sobreviver restart.
//! - Escrita atômica com tmp + rename (replica `O_NOFOLLOW | O_EXCL`
//!   do `.cjs` sem expor a constante em Rust — usamos `write_file_safe`).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{json, Value};

pub const UNLOCK_WINDOW_MS: i64 = 60_000;
pub const UNLOCK_MAX_FAILURES: usize = 5;
pub const UNLOCK_LOCKOUT_MS: i64 = 60_000;

/// Estado compartilhado do rate-limit. Uma instância por servidor.
#[derive(Debug, Default)]
pub struct UnlockGuard {
    inner: Mutex<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    failures: Vec<i64>,
    locked_until: i64,
}

/// Resultado de [`UnlockGuard::check`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimit {
    Allowed,
    Blocked { retry_after_s: i64 },
}

impl UnlockGuard {
    pub fn new() -> Self {
        Self::default()
    }

    /// Carrega estado do disco, se existir.
    pub fn load(mem_dir: &Path) -> Self {
        let g = Self::new();
        let path = state_path(mem_dir);
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return g;
        };
        let v: Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(_) => return g,
        };
        let failures: Vec<i64> = v
            .get("failures")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_i64().or_else(|| x.as_f64().map(|f| f as i64)))
                    .collect()
            })
            .unwrap_or_default();
        let locked_until = v
            .get("locked_until")
            .and_then(Value::as_i64)
            .or_else(|| {
                v.get("locked_until")
                    .and_then(Value::as_f64)
                    .map(|f| f as i64)
            })
            .unwrap_or(0);
        {
            let mut inner = g.inner.lock().unwrap();
            inner.failures = failures;
            inner.locked_until = locked_until;
        }
        g
    }

    /// Checa rate-limit. Se permitido, expira falhas fora da janela.
    pub fn check(&self, now_ms: i64, mem_dir: &Path) -> RateLimit {
        let mut inner = self.inner.lock().unwrap();
        if now_ms < inner.locked_until {
            let retry = ((inner.locked_until - now_ms) as f64 / 1000.0).ceil() as i64;
            return RateLimit::Blocked {
                retry_after_s: retry,
            };
        }
        let cutoff = now_ms - UNLOCK_WINDOW_MS;
        let before = inner.failures.len();
        inner.failures.retain(|t| *t > cutoff);
        let changed = inner.failures.len() != before;
        if changed {
            let snapshot = take_snapshot(&inner);
            drop(inner);
            persist(mem_dir, &snapshot);
        }
        RateLimit::Allowed
    }

    /// Marca falha. Se atinge max, aplica lockout e reseta contagem.
    pub fn record_failure(&self, now_ms: i64, mem_dir: &Path) -> usize {
        let snapshot = {
            let mut inner = self.inner.lock().unwrap();
            inner.failures.push(now_ms);
            if inner.failures.len() >= UNLOCK_MAX_FAILURES {
                inner.locked_until = now_ms + UNLOCK_LOCKOUT_MS;
                inner.failures.clear();
            }
            take_snapshot(&inner)
        };
        persist(mem_dir, &snapshot);
        snapshot.failures.len()
    }

    /// Reseta contagem e remove lockout.
    pub fn reset(&self, mem_dir: &Path) {
        let snapshot = {
            let mut inner = self.inner.lock().unwrap();
            inner.failures.clear();
            inner.locked_until = 0;
            take_snapshot(&inner)
        };
        persist(mem_dir, &snapshot);
    }
}

struct Snapshot {
    failures: Vec<i64>,
    locked_until: i64,
}

fn take_snapshot(inner: &Inner) -> Snapshot {
    Snapshot {
        failures: inner.failures.clone(),
        locked_until: inner.locked_until,
    }
}

fn state_path(mem_dir: &Path) -> PathBuf {
    mem_dir.join(".vault-unlock-state.json")
}

fn persist(mem_dir: &Path, snap: &Snapshot) {
    let path = state_path(mem_dir);
    let payload = json!({
        "failures": snap.failures,
        "locked_until": snap.locked_until,
    });
    let Ok(s) = serde_json::to_string(&payload) else {
        return;
    };
    let _ = std::fs::create_dir_all(mem_dir);
    // tmp + rename atômico (paridade com `.cjs`).
    let rand = format!("{:016x}", rand_u64());
    let tmp = mem_dir.join(format!(".vault-unlock-state.json.tmp-{rand}"));
    if std::fs::write(&tmp, s).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    } else {
        let _ = std::fs::remove_file(&tmp);
    }
}

fn rand_u64() -> u64 {
    // Rand pseudoaleatório baseado em SystemTime — não-crypto, só pra
    // evitar colisão de nome de tmp entre threads. Paridade ok.
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 ^ (d.as_secs().wrapping_mul(6364136223846793005)))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn fresh_allows() {
        let t = TempDir::new().unwrap();
        let g = UnlockGuard::new();
        assert_eq!(g.check(1000, t.path()), RateLimit::Allowed);
    }

    #[test]
    fn five_failures_trigger_lockout() {
        let t = TempDir::new().unwrap();
        let g = UnlockGuard::new();
        for i in 0..5 {
            g.record_failure(1000 + i, t.path());
        }
        let r = g.check(2000, t.path());
        matches!(r, RateLimit::Blocked { .. });
    }

    #[test]
    fn expired_window_drops_failures() {
        let t = TempDir::new().unwrap();
        let g = UnlockGuard::new();
        g.record_failure(1000, t.path());
        g.record_failure(1100, t.path());
        // Janela passou — todas expiram.
        let r = g.check(1_000_000, t.path());
        assert_eq!(r, RateLimit::Allowed);
    }

    #[test]
    fn reset_clears_state() {
        let t = TempDir::new().unwrap();
        let g = UnlockGuard::new();
        for i in 0..5 {
            g.record_failure(i, t.path());
        }
        g.reset(t.path());
        assert_eq!(g.check(10_000, t.path()), RateLimit::Allowed);
    }

    #[test]
    fn load_from_disk_preserves_state() {
        let t = TempDir::new().unwrap();
        let g1 = UnlockGuard::new();
        g1.record_failure(1000, t.path());
        g1.record_failure(2000, t.path());
        let g2 = UnlockGuard::load(t.path());
        let inner = g2.inner.lock().unwrap();
        assert_eq!(inner.failures, vec![1000, 2000]);
    }
}
