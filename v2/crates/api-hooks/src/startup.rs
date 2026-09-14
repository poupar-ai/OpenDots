//! Lógica do hook `SessionStart` — porta de `plugin/scripts/startup.cjs`.
//!
//! O hook roda em `startup | clear | compact` e imprime uma linha de
//! banner em `systemMessage`. O binário em si (`src/bin/startup.rs`) é
//! um thin main que chama [`build_banner`] com inputs reais do ambiente.
//!
//! ### Por que a lib é separada
//!
//! `startup.cjs` mistura três responsabilidades:
//!
//! 1. Montagem da **string de banner** (lógica pura a partir de
//!    `stats.json`, existência de `PROJECT.md`, contagem de agentes).
//! 2. Efeito colateral **spawn do UI server** com pidfile lock.
//! 3. Efeito colateral **warn de MCP não registrado** (read de
//!    `~/.claude/settings.json`).
//!
//! Separamos tudo. O banner é 100% puro e tem paridade byte-a-byte com
//! o `.cjs` sob fixtures idênticas. Os efeitos vivem atrás de helpers
//! injetáveis ([`UiAutostart`], [`McpCheck`]) para o binário real
//! chamar sem o teste precisar tocar disco nem subprocess.
//!
//! ### Paridade JSON
//!
//! O payload final é `{"continue": true, "systemMessage": "<linha>"}`.
//! Serializamos com `serde_json` na mesma ordem do `.cjs`
//! (feature `preserve_order` ativa no `Cargo.toml`) — byte-a-byte igual
//! ao `JSON.stringify` do Node.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

/// Todos os inputs que mudam a string de banner.
///
/// Popular este struct é responsabilidade do main (lê env, memDir,
/// stats.json, etc.). A função [`build_banner`] é puramente
/// determinística sobre ele — fundamental para paridade de teste.
#[derive(Debug, Clone)]
pub struct BannerInputs {
    /// Nome do projeto (último segmento de `projectRoot`). Aparece no banner.
    pub project_name: String,

    /// Se `<mem_dir>` existe no disco. `.cjs` usa `fs.existsSync(memDir)`.
    pub mem_dir_exists: bool,

    /// Conteúdo do `stats.json` se houver — usado pra não reler JSONL.
    /// `.cjs` chama `loadStats(memDir)`; `None` se ausente/corrompido.
    pub stats: Option<Value>,

    /// Fallback: se `stats` for `None` e `memDir` existir, `.cjs` conta
    /// `sessions.jsonl` (filtra `type === 'end'`) e `observations.jsonl`.
    /// O caller pré-calcula esses números.
    pub fallback_sessions: u64,
    pub fallback_observations: u64,

    /// Se `<mem_dir>/PROJECT.md` existe.
    pub has_project_md: bool,

    /// Contagem de agentes declarados (`Object.keys(agents.json.agents).length`).
    /// `None` quando o arquivo não existe ou não parseia — `.cjs` cai no
    /// `catch` silencioso e não incrementa.
    pub agent_count: Option<usize>,

    /// Path para o `seed.cjs` — só entra na string quando
    /// `agent_count <= 2`. O `.cjs` calcula como
    /// `path.join(pluginRoot, 'scripts', 'seed.cjs')`.
    pub seed_cli_path: PathBuf,

    /// Porta da UI usada no link final. `.cjs` lê de
    /// `POUPAR_PORT` com default `37778`.
    pub ui_port: u16,

    /// Warning de MCP ausente, se houver. O caller computa via
    /// [`check_mcp_registered`] — é `Some("..."` com a mensagem
    /// completa (incluindo o trecho " | ⚠ MCP não registrado...").
    pub mcp_warning: Option<String>,
}

/// Monta a linha de banner idêntica à do `.cjs`.
///
/// A string cresce por concatenação nesta ordem:
/// 1. `🧠 open-poupar | <projectName>`
/// 2. Se memDir **não** existe → ` | sem memória ainda — rode /mem-setup...`
/// 3. Senão:
///     - ` | <N> sessões · <M> obs`
///     - ` | PROJECT.md ✓` ou ` | PROJECT.md ✗ — rode /mem-setup`
///     - Se `agent_count <= 2` → ` | 👥 sem time — rode: node <seedCli> setup`
///     - Se `stats.last_injection_tokens` → ` | ~T/Bt` (com sufixo
///       `(⚠X omitidos)` se `skipped.length > 0`)
///     - ` | UI → http://localhost:<port>`
/// 4. [`mcp_warning`](BannerInputs::mcp_warning) anexado (começa com ` | ⚠`).
pub fn build_banner(i: &BannerInputs) -> String {
    let mut line = format!("🧠 open-poupar | {}", i.project_name);

    if !i.mem_dir_exists {
        line.push_str(" | sem memória ainda — rode /mem-setup para iniciar");
    } else {
        // Ordem de pick-up do .cjs: usa stats cache se disponível, senão fallback.
        let (sessions, observations) = match &i.stats {
            Some(s) => (
                s.get("sessions").and_then(Value::as_u64).unwrap_or(0),
                s.get("observations").and_then(Value::as_u64).unwrap_or(0),
            ),
            None => (i.fallback_sessions, i.fallback_observations),
        };

        line.push_str(&format!(" | {sessions} sessões · {observations} obs"));
        line.push_str(if i.has_project_md {
            " | PROJECT.md ✓"
        } else {
            " | PROJECT.md ✗ — rode /mem-setup"
        });

        // Dica de seed: agent_count ≤ 2 conta como "só os defaults".
        // O `.cjs` compara `<= 2` incluindo `0` do catch — replicamos:
        // `None` (arquivo ausente/corrompido) NÃO dispara a dica, porque
        // no `.cjs` o `try { ... } catch {}` mantém `agentCount = 0` mas
        // o bloco todo está dentro do try e o `line +=` só roda no fim.
        // Logo: erro → sem append. Só loga se leu com sucesso e `<= 2`.
        if let Some(c) = i.agent_count {
            if c <= 2 {
                line.push_str(&format!(
                    " | 👥 sem time — rode: node {} setup",
                    i.seed_cli_path.display()
                ));
            }
        }

        // Telemetria do último budget de injeção.
        if let Some(stats) = &i.stats {
            if let Some(tokens) = stats.get("last_injection_tokens").and_then(Value::as_u64) {
                let budget = stats
                    .get("last_injection_budget")
                    .and_then(Value::as_u64)
                    .unwrap_or(4000);
                let skipped = stats
                    .get("last_injection_skipped")
                    .and_then(Value::as_array)
                    .map(|a| a.len())
                    .unwrap_or(0);
                line.push_str(&format!(" | ~{tokens}/{budget}t"));
                if skipped > 0 {
                    line.push_str(&format!(" (⚠{skipped} omitidos)"));
                }
            }
        }

        line.push_str(&format!(" | UI → http://localhost:{}", i.ui_port));
    }

    if let Some(warn) = &i.mcp_warning {
        line.push_str(warn);
    }

    line
}

/// Serializa o payload `{continue, systemMessage}` na ordem do `.cjs`.
///
/// Paridade: `JSON.stringify({continue: true, systemMessage: line})`
/// emite `{"continue":true,"systemMessage":"..."}` sem espaços.
/// `serde_json::to_string` (não pretty) faz o mesmo.
pub fn render_payload(line: &str) -> String {
    let payload = json!({
        "continue": true,
        "systemMessage": line,
    });
    serde_json::to_string(&payload).expect("json sempre serializa")
}

// ─── Helpers I/O para o main ─────────────────────────────────────────

/// Conta `sessions.jsonl` filtrando `type === 'end'` e `observations.jsonl`
/// linha por linha. Fallback só usado quando `stats.json` está ausente.
///
/// Paridade: `readJsonl` do `.cjs` ignora linhas inválidas via
/// `try { JSON.parse } catch { return null }`; aqui idem.
pub fn count_fallback(mem_dir: &Path) -> (u64, u64) {
    let sessions = count_jsonl(&mem_dir.join("sessions.jsonl"), |v| {
        v.get("type").and_then(Value::as_str) == Some("end")
    });
    let observations = count_jsonl(&mem_dir.join("observations.jsonl"), |_| true);
    (sessions, observations)
}

fn count_jsonl<F: Fn(&Value) -> bool>(path: &Path, pred: F) -> u64 {
    let Ok(raw) = fs::read_to_string(path) else {
        return 0;
    };
    raw.lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(pred)
        .count() as u64
}

/// Lê `<mem_dir>/agents.json` e conta entries em `.agents`.
///
/// ### Semântica (replica o `try/catch` do `.cjs`)
///
/// ```js
/// try {
///   let agentCount = 0;
///   if (fs.existsSync(agentsPath)) {
///     const m = JSON.parse(fs.readFileSync(agentsPath, 'utf8')); // pode throw
///     agentCount = Object.keys(m.agents || {}).length;
///   }
///   if (agentCount <= 2) line += ' | 👥 sem time ...';  // dentro do try
/// } catch {}
/// ```
///
/// - Arquivo **ausente** → `Some(0)` — o `.cjs` não entra no `if`
///   `existsSync`, mantém `agentCount = 0` e cai no `<= 2` → dica aparece.
/// - Arquivo **existe e parseia** → `Some(len(agents))`. Sem `.agents`
///   → `Object.keys({}||{}).length = 0`, idem.
/// - Arquivo **existe mas não parseia** (read err ou JSON inválido) →
///   `None`. `.cjs` cai no `catch` ANTES do `line +=`, então a dica
///   **não** aparece. É o único caso em que o resultado "esconde" o hint.
pub fn load_agent_count(mem_dir: &Path) -> Option<usize> {
    let path = mem_dir.join("agents.json");
    if !path.exists() {
        // existsSync = false → agentCount permanece 0 e o if roda: dica aparece.
        return Some(0);
    }
    // Arquivo presente: read + parse. Qualquer erro = exceção swallow → None.
    let raw = fs::read_to_string(&path).ok()?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    // Paridade com `Object.keys(m.agents || {}).length`:
    // - `m.agents` objeto → len dele
    // - `m.agents` ausente/null/não-objeto → 0 (fallback `|| {}`).
    Some(
        v.get("agents")
            .and_then(Value::as_object)
            .map(|o| o.len())
            .unwrap_or(0),
    )
}

/// Verifica `~/.claude/settings.json` por um entry
/// `mcpServers["open-poupar"]`. Retorna a mensagem de warning pronta
/// pra concatenar (começa com ` | ⚠`).
///
/// Paridade do `.cjs`:
/// - `settingsPath = $CLAUDE_CONFIG_DIR/settings.json` ou
///   `$HOME/.claude/settings.json`.
/// - Qualquer exceção no `try/catch` → retorna `None` (nenhum warning).
pub fn check_mcp_registered(settings_path: &Path, mcp_path: &Path) -> Option<String> {
    let raw = fs::read_to_string(settings_path).ok()?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    let already = v
        .get("mcpServers")
        .and_then(|m| m.get("open-poupar"))
        .is_some();
    if already {
        return None;
    }
    Some(format!(
        " | ⚠ MCP não registrado — adicione em settings.json: {{\"mcpServers\":{{\"open-poupar\":{{\"type\":\"stdio\",\"command\":\"node\",\"args\":[\"{}\"]}}}}}}",
        mcp_path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn base_inputs() -> BannerInputs {
        BannerInputs {
            project_name: "demo".into(),
            mem_dir_exists: true,
            stats: None,
            fallback_sessions: 0,
            fallback_observations: 0,
            has_project_md: true,
            agent_count: Some(5),
            seed_cli_path: PathBuf::from("/plugin/scripts/seed.cjs"),
            ui_port: 37778,
            mcp_warning: None,
        }
    }

    #[test]
    fn banner_without_memdir_shows_setup_hint() {
        let mut i = base_inputs();
        i.mem_dir_exists = false;
        let b = build_banner(&i);
        assert_eq!(
            b,
            "🧠 open-poupar | demo | sem memória ainda — rode /mem-setup para iniciar"
        );
    }

    #[test]
    fn banner_with_stats_uses_cache() {
        let mut i = base_inputs();
        i.stats = Some(json!({"sessions": 12, "observations": 99}));
        let b = build_banner(&i);
        assert!(b.contains("12 sessões · 99 obs"));
    }

    #[test]
    fn banner_without_stats_uses_fallback() {
        let mut i = base_inputs();
        i.fallback_sessions = 4;
        i.fallback_observations = 17;
        let b = build_banner(&i);
        assert!(b.contains("4 sessões · 17 obs"));
    }

    #[test]
    fn banner_project_md_cross_when_missing() {
        let mut i = base_inputs();
        i.has_project_md = false;
        let b = build_banner(&i);
        assert!(b.contains("PROJECT.md ✗ — rode /mem-setup"));
    }

    #[test]
    fn banner_seed_hint_when_agent_count_le_two() {
        let mut i = base_inputs();
        i.agent_count = Some(2);
        i.seed_cli_path = PathBuf::from("/p/scripts/seed.cjs");
        let b = build_banner(&i);
        assert!(b.contains("👥 sem time — rode: node /p/scripts/seed.cjs setup"));
    }

    #[test]
    fn banner_no_seed_hint_when_agent_count_missing() {
        // agent_count = None significa exceção (JSON malformado) — o
        // `.cjs` cai no catch ANTES do `line +=`, então sem dica.
        let mut i = base_inputs();
        i.agent_count = None;
        let b = build_banner(&i);
        assert!(!b.contains("sem time"));
    }

    #[test]
    fn load_agent_count_returns_some_zero_when_missing() {
        // Arquivo ausente equivale a `agentCount = 0` no `.cjs`, que
        // dispara a dica (0 <= 2).
        let tmp = TempDir::new().unwrap();
        assert_eq!(load_agent_count(tmp.path()), Some(0));
    }

    #[test]
    fn load_agent_count_returns_none_on_malformed_json() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("agents.json"), "{not json").unwrap();
        assert_eq!(load_agent_count(tmp.path()), None);
    }

    #[test]
    fn load_agent_count_returns_zero_when_no_agents_key() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("agents.json"), r#"{"other":1}"#).unwrap();
        assert_eq!(load_agent_count(tmp.path()), Some(0));
    }

    #[test]
    fn banner_injection_telemetry_with_skipped() {
        let mut i = base_inputs();
        i.stats = Some(json!({
            "sessions": 1,
            "observations": 2,
            "last_injection_tokens": 3200,
            "last_injection_budget": 4000,
            "last_injection_skipped": ["graph:120", "observations:430"]
        }));
        let b = build_banner(&i);
        assert!(b.contains("~3200/4000t (⚠2 omitidos)"));
    }

    #[test]
    fn banner_injection_telemetry_without_skipped() {
        let mut i = base_inputs();
        i.stats = Some(json!({
            "sessions": 1,
            "observations": 2,
            "last_injection_tokens": 1500
        }));
        let b = build_banner(&i);
        // budget default = 4000 quando omisso.
        assert!(b.contains("~1500/4000t |"));
        assert!(!b.contains("omitidos"));
    }

    #[test]
    fn banner_appends_ui_link() {
        let i = base_inputs();
        let b = build_banner(&i);
        assert!(b.contains("UI → http://localhost:37778"));
    }

    #[test]
    fn banner_appends_mcp_warning() {
        let mut i = base_inputs();
        i.mcp_warning = Some(" | ⚠ MCP não registrado — ...".into());
        let b = build_banner(&i);
        assert!(b.ends_with(" | ⚠ MCP não registrado — ..."));
    }

    #[test]
    fn render_payload_emits_compact_json() {
        let s = render_payload("oi");
        assert_eq!(s, r#"{"continue":true,"systemMessage":"oi"}"#);
    }

    #[test]
    fn count_fallback_counts_end_sessions_only() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("sessions.jsonl"),
            "{\"type\":\"start\"}\n{\"type\":\"end\"}\n{\"type\":\"end\"}\n",
        )
        .unwrap();
        fs::write(tmp.path().join("observations.jsonl"), "{}\n{}\n{}\n{}\n").unwrap();
        let (s, o) = count_fallback(tmp.path());
        assert_eq!(s, 2);
        assert_eq!(o, 4);
    }

    #[test]
    fn count_fallback_ignores_invalid_lines() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("sessions.jsonl"),
            "{\"type\":\"end\"}\nnot json\n{\"type\":\"end\"}\n",
        )
        .unwrap();
        let (s, _) = count_fallback(tmp.path());
        assert_eq!(s, 2);
    }

    #[test]
    fn load_agent_count_parses_agents_object() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("agents.json"),
            r#"{"agents":{"a":{},"b":{},"c":{}}}"#,
        )
        .unwrap();
        assert_eq!(load_agent_count(tmp.path()), Some(3));
    }

    #[test]
    fn mcp_check_returns_none_when_already_registered() {
        let tmp = TempDir::new().unwrap();
        let sp = tmp.path().join("settings.json");
        fs::write(&sp, r#"{"mcpServers":{"open-poupar":{"type":"stdio"}}}"#).unwrap();
        assert_eq!(
            check_mcp_registered(&sp, Path::new("/p/mcp/server.cjs")),
            None
        );
    }

    #[test]
    fn mcp_check_returns_warning_when_missing() {
        let tmp = TempDir::new().unwrap();
        let sp = tmp.path().join("settings.json");
        fs::write(&sp, r#"{"mcpServers":{"other":{}}}"#).unwrap();
        let w =
            check_mcp_registered(&sp, Path::new("/p/mcp/server.cjs")).expect("deve haver warning");
        assert!(w.starts_with(" | ⚠ MCP não registrado — "));
        assert!(w.contains("/p/mcp/server.cjs"));
    }

    #[test]
    fn mcp_check_returns_none_when_settings_missing() {
        // Arquivo inexistente → nenhum warning (try/catch do .cjs).
        assert_eq!(
            check_mcp_registered(
                Path::new("/no/such/settings.json"),
                Path::new("/p/mcp/server.cjs"),
            ),
            None
        );
    }
}
