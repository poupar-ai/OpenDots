//! `poupar-hook-observe` — thin entrypoint do `PostToolUse`.
//!
//! Paridade com `plugin/scripts/observe.cjs`:
//! 1. Lê JSON do stdin.
//! 2. Decide via [`api_hooks::observe::decide_observation`].
//! 3. Se `file_edit`, captura diff e anexa à observação.
//! 4. Persiste: `appendJsonl` em `observations.jsonl`, `incrementStats`,
//!    `updateGraph`.
//!
//! Todas as leituras/escritas passam pelos helpers de `core_paths` e
//! `domain_graph` — bin contém apenas o glue, zero lógica testável.

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use api_hooks::observe::{
    augment_file_edit_with_diff, capture_diff, decide_observation, Decision, ObsKind,
};
use core_paths::{
    append_jsonl, ensure_mem_dir, find_project_root, get_cwd, increment_stats,
    read_last_jsonl_line, read_stdin, MEM_DIR_NAME,
};
use domain_graph::{update_graph, SystemClock};

fn main() -> ExitCode {
    let input = read_stdin();

    let cwd = get_cwd(Some(&input));
    let project_root = find_project_root(&cwd);
    let mem_dir: PathBuf = project_root.join(MEM_DIR_NAME);
    if ensure_mem_dir(&mem_dir).is_err() {
        return ExitCode::SUCCESS;
    }

    let ts = now_iso();
    let agent_id = env::var("POUPAR_AGENT")
        .ok()
        .filter(|s| !s.is_empty());

    let decision = decide_observation(&input, &project_root, &ts, agent_id.as_deref());
    let Decision::Record(mut obs) = decision else {
        return ExitCode::SUCCESS;
    };

    if let (ObsKind::FileEdit, Some(rel)) = (obs.kind, obs.rel_file.as_deref()) {
        if let Some(diff) = capture_diff(&project_root, rel) {
            augment_file_edit_with_diff(&mut obs.value, &diff);
        }
    }

    persist(&mem_dir, &obs.value);
    ExitCode::SUCCESS
}

fn persist(mem_dir: &Path, obs: &serde_json::Value) {
    let obs_file = mem_dir.join("observations.jsonl");
    let prev = read_last_jsonl_line(&obs_file).ok().flatten();
    let _ = append_jsonl(&obs_file, obs);
    let _ = increment_stats(mem_dir, "observations");
    let _ = update_graph(mem_dir, obs, prev.as_ref(), &SystemClock);
}

/// Formato ISO 8601 com milissegundos em UTC: `YYYY-MM-DDTHH:MM:SS.sssZ`.
/// Paridade com `new Date().toISOString()` do Node.
fn now_iso() -> String {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    domain_graph::format_iso_millis(ms)
}
