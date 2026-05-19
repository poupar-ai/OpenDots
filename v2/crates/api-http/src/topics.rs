//! Topic-based pub/sub — storage helpers.
//!
//! Estrutura em disco:
//! ```text
//! <mem_dir>/topics/
//!   <name>/
//!     subscribers.json   # {"agents": ["manager", "oracle", ...]}
//!     events.jsonl       # append: {ts, from, message, msg_id, delivered_to}
//! ```
//!
//! Topics são criados implicitamente no primeiro subscribe ou publish.
//! Sem subscriber, publish entrega pra ninguém mas grava no log mesmo
//! assim (audit de "alguém tentou publicar mas não tinha audiência").

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Subscribers {
    pub agents: BTreeSet<String>,
}

pub fn topics_dir(mem_dir: &Path) -> PathBuf {
    mem_dir.join("topics")
}

pub fn topic_dir(mem_dir: &Path, name: &str) -> PathBuf {
    topics_dir(mem_dir).join(name)
}

pub fn subscribers_file(mem_dir: &Path, name: &str) -> PathBuf {
    topic_dir(mem_dir, name).join("subscribers.json")
}

pub fn events_file(mem_dir: &Path, name: &str) -> PathBuf {
    topic_dir(mem_dir, name).join("events.jsonl")
}

/// Topic name precisa ser kebab/snake — sem barras, sem dots, sem espaço.
/// Evita path traversal e ambiguidade no filesystem.
pub fn is_valid_topic_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 64 {
        return false;
    }
    name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub fn load_subscribers(mem_dir: &Path, name: &str) -> Subscribers {
    let content = match fs::read_to_string(subscribers_file(mem_dir, name)) {
        Ok(c) => c,
        Err(_) => return Subscribers::default(),
    };
    serde_json::from_str(&content).unwrap_or_default()
}

pub fn save_subscribers(
    mem_dir: &Path,
    name: &str,
    subs: &Subscribers,
) -> std::io::Result<()> {
    let dir = topic_dir(mem_dir, name);
    fs::create_dir_all(&dir)?;
    let json = serde_json::to_string_pretty(subs).unwrap_or_else(|_| "{}".into());
    let tmp = dir.join("subscribers.json.tmp");
    fs::write(&tmp, json)?;
    fs::rename(tmp, subscribers_file(mem_dir, name))
}

pub fn append_event(mem_dir: &Path, name: &str, ev: &serde_json::Value) -> std::io::Result<()> {
    let dir = topic_dir(mem_dir, name);
    fs::create_dir_all(&dir)?;
    let line = serde_json::to_string(ev).unwrap_or_default();
    use std::io::Write;
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(events_file(mem_dir, name))?;
    writeln!(f, "{line}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topic_name_aceita_kebab_snake_alfa() {
        assert!(is_valid_topic_name("ci-events"));
        assert!(is_valid_topic_name("alerts_v2"));
        assert!(is_valid_topic_name("a"));
        assert!(is_valid_topic_name("ABC123"));
    }

    #[test]
    fn topic_name_rejeita_traversal_e_caracteres_perigosos() {
        assert!(!is_valid_topic_name(""), "vazio");
        assert!(!is_valid_topic_name("../etc"), "path traversal");
        assert!(!is_valid_topic_name("a/b"), "barra");
        assert!(!is_valid_topic_name("a b"), "espaço");
        assert!(!is_valid_topic_name("a.b"), "ponto (esconde dotfiles)");
        assert!(!is_valid_topic_name(".hidden"), "começa com dot");
        assert!(!is_valid_topic_name("ola!"), "char especial");
        let long: String = "x".repeat(65);
        assert!(!is_valid_topic_name(&long), "limite 64 chars");
    }

    #[test]
    fn subscribers_roundtrip_grava_e_le() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut subs = Subscribers::default();
        subs.agents.insert("manager".into());
        subs.agents.insert("oracle".into());
        save_subscribers(tmp.path(), "ci-events", &subs).unwrap();
        let loaded = load_subscribers(tmp.path(), "ci-events");
        assert_eq!(loaded.agents.len(), 2);
        assert!(loaded.agents.contains("manager"));
        assert!(loaded.agents.contains("oracle"));
    }

    #[test]
    fn list_topics_so_lista_dirs_validos() {
        let tmp = tempfile::TempDir::new().unwrap();
        // Cria 3 topics + 1 arquivo solto + 1 hidden — só os 3 dirs
        // válidos devem aparecer.
        for t in ["alerts", "ci-events", "deploys"] {
            save_subscribers(tmp.path(), t, &Subscribers::default()).unwrap();
        }
        std::fs::write(tmp.path().join("topics").join("not-a-dir.txt"), "x").ok();
        std::fs::create_dir_all(tmp.path().join("topics").join(".hidden")).ok();
        let mut topics = list_topics(tmp.path());
        topics.sort();
        assert_eq!(topics, vec!["alerts", "ci-events", "deploys"]);
    }
}

pub fn list_topics(mem_dir: &Path) -> Vec<String> {
    let dir = topics_dir(mem_dir);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|s| is_valid_topic_name(s))
        .collect();
    out.sort();
    out
}
