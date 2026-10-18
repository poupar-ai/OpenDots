//! # domain-agents
//!
//! Porta Rust de `plugin/scripts/agents.cjs` — **registry de agentes**
//! por projeto. Não orquestra: declara papéis (Manager, Oracle,
//! specialists, integrators, motores, firefighters) e produz um bloco
//! de markdown injetado no contexto do Claude Code para que os
//! subagents nativos sejam usados com consistência.
//!
//! ## Escopo (Fase 3, ponto "agents")
//!
//! Este crate é porta literal — mesma API pública do `.cjs`, mesmos
//! formatos em disco, mesmo comportamento de erro/fallback. Paridade
//! byte-a-byte validada por `tests/parity_*.rs` contra o `.cjs`
//! original.
//!
//! ### Sub-pontos portados
//!
//! - **A** — constants + path helpers ([`MANIFEST`], [`DIR`],
//!   [`AGENT_TYPES`], [`AUTONOMY_LEVELS`], [`manifest_path`],
//!   [`agents_dir`]).
//! - **B** — manifest I/O ([`load_manifest`], [`save_manifest`]).
//! - **C** — mini-context I/O ([`load_mini_context`],
//!   [`save_mini_context`], [`MAX_LENGTH_UTF16`]) + helper
//!   `js_trim` para paridade com `.trim()` do JS.
//!
//! - **D** — defaults + initDefaults ([`default_manifest`],
//!   [`default_mini_contexts`], [`init_defaults`]).
//! - **E** — countInvocations ([`count_invocations`]).
//! - **F** — buildInjectionBlock ([`build_injection_block`]).
//! - **J** — scope/tools ([`KNOWN_TOOL_NAMES`], [`is_known_tool`],
//!   [`sanitize_tools`], [`sanitize_scope`], [`update_scope`]).
//! - **G** — add/remove agent ([`add_agent`], [`remove_agent`]).
//! - **H** — autonomy ([`normalize_autonomy`], [`update_autonomy`],
//!   [`propagate_autonomy_to_seed`]).
//! - **I** — model ([`normalize_model`], [`resolve_cli_model`],
//!   [`update_model`], [`propagate_model_to_seed`], [`CLI_MODEL_MAP`]).
//! - **K** — vault access ([`SECRET_NAME_RE`], [`is_valid_secret_name`],
//!   [`sanitize_secret_names`], [`update_vault_access`]).
//! - **L** — templates ([`integrator_template`], [`firefighter_template`],
//!   [`motor_template`]).
//!
//! **Fase 3 (agents.cjs) completa.** Todos os sub-pontos A–L portados.
//!
//! ## Estrutura em disco
//!
//! - `<mem_dir>/agents.json` — manifest estruturado (versão, manager,
//!   oracle, rule, agents map).
//! - `<mem_dir>/agents/<nome>.md` — mini-contexto de cada agente.

use std::path::{Path, PathBuf};

pub mod manifest;
pub use manifest::{load_manifest, save_manifest};

pub(crate) mod js_trim;

pub mod mini_context;
pub use mini_context::{
    load_mini_context, save_mini_context, MiniContextError, MAX_LENGTH_UTF16,
};

pub mod defaults;
pub use defaults::{default_manifest, default_mini_contexts, init_defaults};

pub mod invocations;
pub use invocations::count_invocations;

pub mod injection;
pub use injection::build_injection_block;

pub mod scope;
pub use scope::{
    is_known_tool, sanitize_scope, sanitize_tools, update_scope, UpdateScopeError,
    KNOWN_TOOL_NAMES,
};

pub mod registry;
pub use registry::{add_agent, remove_agent, AddAgentError, RemoveAgentError};

pub mod autonomy;
pub use autonomy::{
    normalize_autonomy, propagate_autonomy_to_seed, update_autonomy, NormalizeAutonomyError,
    PropagateAutonomyError, UpdateAutonomyError,
};

pub mod model;
pub use model::{
    normalize_model, propagate_model_to_seed, resolve_cli_model, update_model,
    NormalizeModelError, PropagateModelError, UpdateModelError, CLI_MODEL_MAP, DEFAULT_MODEL,
};

pub mod vault;
pub use vault::{
    is_valid_secret_name, sanitize_secret_names, update_vault_access,
    UpdateVaultAccessError, SECRET_NAME_RE,
};

pub mod templates;
pub use templates::{firefighter_template, integrator_template, motor_template};

/// Nome do arquivo de manifest dentro do `mem_dir`.
///
/// Replica `MANIFEST = 'agents.json'` em `agents.cjs:14`.
pub const MANIFEST: &str = "agents.json";

/// Nome do sub-diretório que guarda os mini-contextos (`*.md`).
///
/// Replica `DIR = 'agents'` em `agents.cjs:15`.
pub const DIR: &str = "agents";

/// Tipos válidos de agente. Usado por [`is_valid_agent_type`] e pela
/// validação de [`crate::add_agent`] (sub-ponto G).
///
/// Ordem preservada do `.cjs` para que eventual iteração seja
/// determinística (Rust `Set` seria mais idiomático, mas mudar para
/// `HashSet` quebraria paridade em qualquer serialização).
///
/// - `core` — Manager, Oracle (sempre existem).
/// - `specialist` — conhecimento interno (architect, reviewer, etc.).
/// - `integrator` — API externa, informa sem agir. Sem MCPs.
/// - `motor` — age via MCPs. Guardrails obrigatórios.
/// - `firefighter` — reativo, escuta logs, triage via `claude -p`.
pub const AGENT_TYPES: &[&str] = &["core", "specialist", "integrator", "motor", "firefighter"];

/// Níveis válidos de autonomia.
///
/// - `low` — escala quase tudo.
/// - `medium` — decide em tarefa clara/reversível.
/// - `high` — decide e executa, só escala em casos listados.
pub const AUTONOMY_LEVELS: &[&str] = &["low", "medium", "high"];

/// `true` se `value` é um tipo de agente reconhecido.
///
/// Wrapper sobre [`AGENT_TYPES`] para caller externo não precisar
/// iterar. Replica `AGENT_TYPES.has(value)` do `.cjs`.
pub fn is_valid_agent_type(value: &str) -> bool {
    AGENT_TYPES.contains(&value)
}

/// `true` se `value` é um nível de autonomia reconhecido.
///
/// Replica `AUTONOMY_LEVELS.has(value)` do `.cjs`.
pub fn is_valid_autonomy_level(value: &str) -> bool {
    AUTONOMY_LEVELS.contains(&value)
}

/// Resolve `<mem_dir>/agents.json`.
///
/// Replica `manifestPath(memDir)` em `agents.cjs:408`. Puramente
/// sintático — não toca disco.
pub fn manifest_path(mem_dir: &Path) -> PathBuf {
    mem_dir.join(MANIFEST)
}

/// Resolve `<mem_dir>/agents/`.
///
/// Replica `agentsDir(memDir)` em `agents.cjs:409`.
pub fn agents_dir(mem_dir: &Path) -> PathBuf {
    mem_dir.join(DIR)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_constant_matches_cjs() {
        assert_eq!(MANIFEST, "agents.json");
    }

    #[test]
    fn dir_constant_matches_cjs() {
        assert_eq!(DIR, "agents");
    }

    #[test]
    fn agent_types_set_is_complete() {
        assert!(is_valid_agent_type("core"));
        assert!(is_valid_agent_type("specialist"));
        assert!(is_valid_agent_type("integrator"));
        assert!(is_valid_agent_type("motor"));
        assert!(is_valid_agent_type("firefighter"));
    }

    #[test]
    fn agent_types_rejects_unknown() {
        assert!(!is_valid_agent_type("unknown"));
        assert!(!is_valid_agent_type(""));
        assert!(!is_valid_agent_type("CORE"), "case-sensitive");
    }

    #[test]
    fn autonomy_levels_set_is_complete() {
        assert!(is_valid_autonomy_level("low"));
        assert!(is_valid_autonomy_level("medium"));
        assert!(is_valid_autonomy_level("high"));
    }

    #[test]
    fn autonomy_levels_rejects_unknown() {
        assert!(!is_valid_autonomy_level("auto"));
        assert!(!is_valid_autonomy_level(""));
        assert!(!is_valid_autonomy_level("HIGH"), "case-sensitive");
    }

    #[test]
    fn manifest_path_joins_constant() {
        let got = manifest_path(Path::new("/m"));
        assert_eq!(got, PathBuf::from("/m/agents.json"));
    }

    #[test]
    fn agents_dir_joins_constant() {
        let got = agents_dir(Path::new("/m"));
        assert_eq!(got, PathBuf::from("/m/agents"));
    }
}
