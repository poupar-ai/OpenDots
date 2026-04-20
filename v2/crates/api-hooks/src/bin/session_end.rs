//! `pmem-hook-session-end` — thin entrypoint do `SessionEnd`.
//!
//! Paridade com `plugin/scripts/session-end.cjs`:
//! 1. Lê stdin JSON com `session_id`.
//! 2. Se observations.jsonl ausente → regenera MDs + exit.
//! 3. Carrega obs filtradas por session; se vazio → regenera MDs + exit.
//! 4. Monta summary + classifica sessão + detecta gotchas.
//! 5. Append `sessions.jsonl` (end record) + incrementa stats.
//! 6. Regenera DECISIONS/GOTCHAS/GLOSSARY.md.
//! 7. Update incremental do code index (domain-indexer).
//! 8. Remove flag `tmp/.init-<session_id>`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use api_hooks::session_end::{
    build_session_end_record, build_summary, classify_session, detect_gotchas, filter_by_session,
    render_decisions_md, render_glossary_md, render_gotchas_md, session_changed_files,
};
use core_paths::{
    append_jsonl, ensure_mem_dir, find_project_root, get_cwd, increment_stats, read_jsonl,
    read_stdin, tmp_dir, MEM_DIR_NAME,
};
use domain_graph::{format_iso_millis, on_gotcha};
use domain_indexer::{update_index_for_files, SystemClock as IndexerClock};
use serde_json::Value;

fn main() -> ExitCode {
    let input = read_stdin();
    let session_id = input
        .get("session_id")
        .and_then(Value::as_str)
        .map(|s| s.to_string());

    let cwd = get_cwd(Some(&input));
    let project_root = find_project_root(&cwd);
    let mem_dir: PathBuf = project_root.join(MEM_DIR_NAME);
    if ensure_mem_dir(&mem_dir).is_err() {
        return ExitCode::SUCCESS;
    }

    let obs_file = mem_dir.join("observations.jsonl");
    if !obs_file.exists() {
        regenerate_mds(&mem_dir);
        return ExitCode::SUCCESS;
    }

    let all_obs = read_jsonl(&obs_file).unwrap_or_default();
    let session_obs: Vec<Value> = match &session_id {
        Some(sid) => filter_by_session(&all_obs, sid),
        None => Vec::new(),
    };
    if session_obs.is_empty() {
        regenerate_mds(&mem_dir);
        return ExitCode::SUCCESS;
    }

    let summary = build_summary(&session_obs);
    let kind = classify_session(&session_obs);
    let ts = now_iso();

    if let Some(sid) = session_id.as_deref() {
        let gotchas = detect_gotchas(&session_obs, sid, &ts);
        if !gotchas.is_empty() {
            let gotchas_file = mem_dir.join("gotchas.jsonl");
            for g in &gotchas {
                let _ = append_jsonl(&gotchas_file, g);
                on_gotcha(&mem_dir, g, &domain_graph::SystemClock);
            }
        }
    }

    let record = build_session_end_record(
        session_id.as_deref(),
        &ts,
        session_obs.len(),
        kind,
        &summary,
    );
    let sess_file = mem_dir.join("sessions.jsonl");
    let _ = append_jsonl(&sess_file, &record);
    let _ = increment_stats(&mem_dir, "sessions");

    regenerate_mds(&mem_dir);

    let changed_files = session_changed_files(&session_obs);
    if !changed_files.is_empty() {
        let _ = update_index_for_files(&mem_dir, &project_root, &changed_files, &IndexerClock);
    }

    if let Some(sid) = session_id {
        let flag = tmp_dir(&mem_dir).join(format!(".init-{sid}"));
        if flag.exists() {
            let _ = fs::remove_file(&flag);
        }
    }

    ExitCode::SUCCESS
}

fn regenerate_mds(mem_dir: &Path) {
    let decisions_file = mem_dir.join("decisions.jsonl");
    if decisions_file.exists() {
        let decisions = read_jsonl(&decisions_file).unwrap_or_default();
        if let Some(md) = render_decisions_md(&decisions) {
            let _ = fs::write(mem_dir.join("DECISIONS.md"), md);
        }
    }

    let gotchas_file = mem_dir.join("gotchas.jsonl");
    if gotchas_file.exists() {
        let gotchas = read_jsonl(&gotchas_file).unwrap_or_default();
        if let Some(md) = render_gotchas_md(&gotchas) {
            let _ = fs::write(mem_dir.join("GOTCHAS.md"), md);
        }
    }

    let glossary_file = mem_dir.join("glossary.jsonl");
    if glossary_file.exists() {
        let entries = read_jsonl(&glossary_file).unwrap_or_default();
        if let Some(md) = render_glossary_md(&entries) {
            let _ = fs::write(mem_dir.join("GLOSSARY.md"), md);
        }
    }
}

fn now_iso() -> String {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    format_iso_millis(ms)
}
