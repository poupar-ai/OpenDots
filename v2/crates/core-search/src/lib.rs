//! Search semântica (BM25 via SQLite FTS5) sobre chats/observations.
//!
//! Hermes Agent fez essa decisão antes da gente — vide ADR-001 e a
//! análise comparativa: search era nossa lacuna real, agora cobrimos.
//!
//! ## Arquitetura
//!
//! - SQLite com tabela virtual FTS5 (`docs`). 1 arquivo em
//!   `<mem_dir>/search.db`. Bundled — sem dep no system lib.
//! - Build at startup via `index_jsonl_dir` (rebuild completo).
//! - Update incremental via `add(doc)` chamado por hot paths
//!   (append_chat, append_observation).
//! - Query via `search(q, limit, filter)` retorna hits com BM25 score
//!   + snippet com highlight.
//!
//! ## Thread-safety
//!
//! `SearchIndex` é `Send + Sync` via `Mutex<Connection>`. Hot path de
//! `add` faz INSERT rápido (~ms). Search é POST esporádico — não
//! bloqueia muito. Caso vire gargalo, migrar pra connection pool
//! (`r2d2_sqlite`) ou ler-rwlock.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Um documento indexável. `source_type` identifica origem (chat,
/// observation, session, etc) — usado tanto pra filtro em queries
/// quanto pra reconstruir referência original via `ref_id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchDoc {
    pub content: String,
    pub source_type: String,
    /// Identificador interno do source — pra chat = nome do agente,
    /// pra observation = session_id, etc. Permite voltar do hit pro
    /// arquivo/linha original se quiser.
    pub ref_id: String,
    /// Timestamp ISO ou epoch ms — não indexável, mas retornável.
    pub ts: String,
}

/// Resultado de uma query. `score` é BM25 (menor = mais relevante,
/// SQLite FTS5 retorna negativo — então invertemos: score=0 = mais
/// relevante, valores maiores = menos relevante).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub score: f64,
    pub content: String,
    pub snippet: String,
    pub source_type: String,
    pub ref_id: String,
    pub ts: String,
}

pub struct SearchIndex {
    conn: Mutex<Connection>,
}

impl SearchIndex {
    /// Abre/cria o banco em `<mem_dir>/search.db`. Cria tabela FTS5
    /// `docs` se não existe.
    pub fn open(mem_dir: &Path) -> Result<Self, SearchError> {
        std::fs::create_dir_all(mem_dir)?;
        let conn = Connection::open(mem_dir.join("search.db"))?;
        // FTS5: BM25 ranking nativo. UNINDEXED em ref_id/ts pra
        // economizar espaço (não buscamos por essas colunas).
        conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS docs USING fts5(
                content,
                source_type UNINDEXED,
                ref_id UNINDEXED,
                ts UNINDEXED,
                tokenize='unicode61 remove_diacritics 2'
            );",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Adiciona 1 doc ao índice. Hot path — chamado após cada append
    /// de chat/observation. ~1ms por insert.
    pub fn add(&self, doc: &SearchDoc) -> Result<(), SearchError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO docs (content, source_type, ref_id, ts) VALUES (?1, ?2, ?3, ?4)",
            params![doc.content, doc.source_type, doc.ref_id, doc.ts],
        )?;
        Ok(())
    }

    /// Adiciona N docs numa transação. ~10× mais rápido que loop de
    /// `add` em bulk (rebuild at startup, batch import).
    pub fn add_batch(&self, docs: &[SearchDoc]) -> Result<(), SearchError> {
        if docs.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO docs (content, source_type, ref_id, ts) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for doc in docs {
                stmt.execute(params![
                    doc.content,
                    doc.source_type,
                    doc.ref_id,
                    doc.ts
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Limpa todos os docs. Usado antes de rebuild completo.
    pub fn clear(&self) -> Result<(), SearchError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM docs", [])?;
        Ok(())
    }

    /// Query BM25. `q` é texto livre (FTS5 syntax — palavras separadas
    /// por espaço = AND, `"frase exata"`, `palavra*` = prefix, `NOT`
    /// pra negação, etc.). `limit` cap em 200 pra evitar resposta
    /// pesada. `source_filter` opcional.
    pub fn search(
        &self,
        q: &str,
        limit: usize,
        source_filter: Option<&str>,
    ) -> Result<Vec<SearchHit>, SearchError> {
        let limit = limit.min(200).max(1);
        let conn = self.conn.lock().unwrap();
        let q_escaped = escape_fts_query(q);
        // ?-style positional pra cada param dinâmico — índices se
        // ajustam com presença/ausência do filtro.
        let mut sql = String::from(
            "SELECT bm25(docs) AS score, content, snippet(docs, 0, '<mark>', '</mark>', '…', 16) AS snip, source_type, ref_id, ts \
             FROM docs WHERE docs MATCH ?",
        );
        if source_filter.is_some() {
            sql.push_str(" AND source_type = ?");
        }
        sql.push_str(" ORDER BY score LIMIT ?");

        let mut stmt = conn.prepare(&sql)?;
        let hits: Vec<SearchHit> = if let Some(sf) = source_filter {
            stmt.query_map(params![q_escaped, sf, limit as i64], row_to_hit)?
                .collect::<Result<Vec<_>, _>>()?
        } else {
            stmt.query_map(params![q_escaped, limit as i64], row_to_hit)?
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(hits)
    }

    /// Quantos docs estão indexados — telemetria.
    pub fn count(&self) -> Result<usize, SearchError> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM docs", [], |r| r.get(0))?;
        Ok(n as usize)
    }
}

fn row_to_hit(row: &rusqlite::Row) -> rusqlite::Result<SearchHit> {
    // bm25 do SQLite FTS5 retorna VALORES NEGATIVOS pra docs mais
    // relevantes. Invertemos pra ficar "score=0 → mais relevante,
    // maior = menos relevante" (mais intuitivo no JSON pra cliente).
    let raw_score: f64 = row.get(0)?;
    Ok(SearchHit {
        score: -raw_score,
        content: row.get(1)?,
        snippet: row.get(2)?,
        source_type: row.get(3)?,
        ref_id: row.get(4)?,
        ts: row.get(5)?,
    })
}

/// Escape pra FTS5: cita aspas duplas pra evitar parse error em queries
/// "naïve" do usuário (ex.: `foo's bar` → `"foo's bar"`). Caller que
/// quer usar sintaxe FTS5 avançada (NEAR, OR, prefix) precisa passar
/// já escapado e usar API direta — esse helper trata texto comum.
fn escape_fts_query(q: &str) -> String {
    // Se tem operadores FTS5 (AND/OR/NOT/NEAR/", -), assumimos query
    // estruturada e passamos como está. Senão, quotamos.
    let has_operator = q
        .split_whitespace()
        .any(|w| matches!(w, "AND" | "OR" | "NOT" | "NEAR"))
        || q.contains('"')
        || q.contains('*');
    if has_operator {
        q.to_string()
    } else {
        // Quote individual tokens pra evitar sintaxe acidental.
        let tokens: Vec<String> = q
            .split_whitespace()
            .map(|w| format!("\"{}\"", w.replace('"', "")))
            .collect();
        tokens.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn doc(content: &str, src: &str, refid: &str) -> SearchDoc {
        SearchDoc {
            content: content.into(),
            source_type: src.into(),
            ref_id: refid.into(),
            ts: "2026-05-18T20:00:00Z".into(),
        }
    }

    #[test]
    fn add_then_search_finds_match() {
        let tmp = TempDir::new().unwrap();
        let idx = SearchIndex::open(tmp.path()).unwrap();
        idx.add(&doc("preciso refatorar o handoff handler", "chat", "manager")).unwrap();
        idx.add(&doc("vault destrancado às 14h", "observation", "session-1")).unwrap();
        idx.add(&doc("nenhuma palavra em comum", "chat", "oracle")).unwrap();
        let hits = idx.search("handoff", 10, None).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].content.contains("refatorar o handoff"));
        assert!(hits[0].snippet.contains("<mark>handoff</mark>"));
    }

    #[test]
    fn batch_insert_ordering_by_relevance() {
        let tmp = TempDir::new().unwrap();
        let idx = SearchIndex::open(tmp.path()).unwrap();
        let docs = vec![
            doc("uma menção rápida de rust em meio a um texto longo", "chat", "a"),
            doc("rust rust rust quase nada mais", "chat", "b"),
            doc("texto totalmente unrelated", "chat", "c"),
        ];
        idx.add_batch(&docs).unwrap();
        let hits = idx.search("rust", 10, None).unwrap();
        assert_eq!(hits.len(), 2);
        // BM25 — "b" tem mais ocorrências em texto curto → mais relevante.
        assert_eq!(hits[0].ref_id, "b");
        assert_eq!(hits[1].ref_id, "a");
    }

    #[test]
    fn filter_by_source_type() {
        let tmp = TempDir::new().unwrap();
        let idx = SearchIndex::open(tmp.path()).unwrap();
        idx.add(&doc("alpha", "chat", "x")).unwrap();
        idx.add(&doc("alpha", "observation", "y")).unwrap();
        let only_chat = idx.search("alpha", 10, Some("chat")).unwrap();
        assert_eq!(only_chat.len(), 1);
        assert_eq!(only_chat[0].source_type, "chat");
        let only_obs = idx.search("alpha", 10, Some("observation")).unwrap();
        assert_eq!(only_obs.len(), 1);
        assert_eq!(only_obs[0].source_type, "observation");
    }

    #[test]
    fn malformed_query_doesnt_panic() {
        let tmp = TempDir::new().unwrap();
        let idx = SearchIndex::open(tmp.path()).unwrap();
        idx.add(&doc("foo bar baz", "chat", "x")).unwrap();
        // Queries com apostrophe/punctuation — historicamente quebravam
        // FTS5. O escape_fts_query trata isso.
        let _ = idx.search("foo's bar", 10, None).unwrap();
        let _ = idx.search("a:b", 10, None).unwrap();
    }

    #[test]
    fn count_reflects_inserts() {
        let tmp = TempDir::new().unwrap();
        let idx = SearchIndex::open(tmp.path()).unwrap();
        assert_eq!(idx.count().unwrap(), 0);
        idx.add(&doc("x", "chat", "a")).unwrap();
        idx.add(&doc("y", "chat", "b")).unwrap();
        assert_eq!(idx.count().unwrap(), 2);
        idx.clear().unwrap();
        assert_eq!(idx.count().unwrap(), 0);
    }
}
