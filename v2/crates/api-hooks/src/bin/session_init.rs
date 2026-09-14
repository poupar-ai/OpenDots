//! `poupar-hook-session-init` — thin entrypoint do `UserPromptSubmit`.
//!
//! Paridade com `plugin/scripts/session-init.cjs`:
//! 1. Lê stdin (session_id, prompt, cwd).
//! 2. Detecta correções do usuário em **todos** os turnos (não só no primeiro).
//! 3. Se já houve injeção nesta sessão (`tmp/.init-<sid>` existe) → sai calado.
//! 4. Primeira vez no projeto (sem PROJECT.md e sem sessions.jsonl)
//!    → `init_project_config`, append start record, retorna onboarding.
//! 5. Caso contrário: append start record, sync SOUL+PROJECT → CLAUDE.md,
//!    monta 12 blocos com prioridade, aplica token budget, persiste
//!    métricas em stats.json, emite payload com `additionalContext`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{SystemTime, UNIX_EPOCH};

use api_hooks::session_init::{
    adr_title_from_content, apply_token_budget, build_correction_record, format_adrs_block,
    format_corrections_block, format_decisions_log_block, format_git_block,
    format_gotchas_log_block, format_observations_block, format_sessions_block,
    is_correction_marker, onboarding_message, render_final_context, render_hook_payload,
    render_md_block, render_session_start_record, AdrEntry, Block, GitContext, RecentCommit,
};
use core_paths::{
    append_jsonl, ensure_mem_dir, ensure_tmp_dir, estimate_tokens, find_project_root, get_cwd,
    init_project_config, load_stats, read_jsonl, read_last_jsonl_line, read_stdin, save_stats,
    sync_claude_md, tmp_dir, MEM_DIR_NAME,
};
use domain_agents::build_injection_block;
use domain_graph::{format_iso_millis, get_context_lines as graph_context_lines, load_graph};
use domain_indexer::get_context_lines as indexer_context_lines;
use serde_json::{json, Value};

fn main() -> ExitCode {
    let input = read_stdin();
    let session_id = input
        .get("session_id")
        .and_then(Value::as_str)
        .map(|s| s.to_string());
    let user_prompt = input
        .get("prompt")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let cwd = get_cwd(Some(&input));
    let project_root = find_project_root(&cwd);
    let project_name = project_root
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let md: PathBuf = project_root.join(MEM_DIR_NAME);

    if ensure_mem_dir(&md).is_err() {
        println!("{}", render_hook_payload(None));
        return ExitCode::SUCCESS;
    }
    let _ = ensure_tmp_dir(&md);

    let soul_md = md.join("SOUL.md");
    let project_md = md.join("PROJECT.md");
    let obs_file = md.join("observations.jsonl");
    let sess_file = md.join("sessions.jsonl");
    let decisions_file = md.join("decisions.jsonl");
    let corrections_file = md.join("corrections.jsonl");
    let gotchas_file = md.join("gotchas.jsonl");
    let decisions_md = md.join("DECISIONS.md");
    let gotchas_md = md.join("GOTCHAS.md");
    let glossary_md = md.join("GLOSSARY.md");

    let ts = now_iso();

    // 1. Detector de correções (todo turno).
    if is_correction_marker(&user_prompt) {
        let last_obs = read_last_jsonl_line(&obs_file).ok().flatten();
        let previous = match (&last_obs, &session_id) {
            (Some(o), Some(sid)) => {
                if o.get("session_id").and_then(Value::as_str) == Some(sid.as_str()) {
                    Some(o.clone())
                } else {
                    None
                }
            }
            _ => None,
        };
        let rec =
            build_correction_record(session_id.as_deref(), &user_prompt, previous.as_ref(), &ts);
        let _ = append_jsonl(&corrections_file, &rec);
    }

    // 2. Idempotência por sessão.
    let flag_file: Option<PathBuf> = session_id
        .as_deref()
        .map(|sid| tmp_dir(&md).join(format!(".init-{sid}")));
    if let Some(flag) = &flag_file {
        if flag.exists() {
            println!("{}", render_hook_payload(None));
            return ExitCode::SUCCESS;
        }
        let _ = fs::write(flag, "");
    }

    // 3. Primeira vez neste projeto.
    let first_time = !project_md.exists() && !sess_file.exists();
    if first_time {
        let _ = init_project_config(&md, &project_name, &ts);
        let start = render_session_start_record(session_id.as_deref(), &ts, &cwd.to_string_lossy());
        let _ = append_jsonl(&sess_file, &start);
        println!("{}", render_hook_payload(Some(onboarding_message())));
        return ExitCode::SUCCESS;
    }

    // 4. Sessões subsequentes: append start record.
    let start = render_session_start_record(session_id.as_deref(), &ts, &cwd.to_string_lossy());
    let _ = append_jsonl(&sess_file, &start);

    // 5. Sync SOUL+PROJECT → .claude/CLAUDE.md.
    let soul_content = read_to_trimmed(&soul_md);
    let project_content = read_to_trimmed(&project_md);
    let pieces: Vec<&str> = [&soul_content, &project_content]
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| s.as_str())
        .collect();
    let native_block = pieces.join("\n\n---\n\n");
    if !native_block.is_empty() {
        let _ = sync_claude_md(&project_root, &native_block);
    }

    // 6. Monta blocos com prioridade.
    let mut blocks: Vec<Block> = Vec::new();

    if !soul_content.is_empty() {
        push_block(&mut blocks, "soul", 1, &soul_content);
    }
    if !project_content.is_empty() {
        push_block(&mut blocks, "project", 2, &project_content);
    }

    for (name, title, file) in [
        ("decisions_md", "## Decisões registradas", &decisions_md),
        ("gotchas_md", "## Armadilhas conhecidas", &gotchas_md),
        ("glossary_md", "## Glossário do projeto", &glossary_md),
    ] {
        if !file.exists() {
            continue;
        }
        let content = fs::read_to_string(file).unwrap_or_default();
        if let Some(lines) = render_md_block(title, &content) {
            push_block(&mut blocks, name, 3, &lines.join("\n"));
        }
    }

    let agent_lines = build_injection_block(&md);
    if !agent_lines.is_empty() {
        push_block(&mut blocks, "agents", 4, &agent_lines.join("\n"));
    }

    if let Some(git) = read_git_context(&project_root) {
        let lines = format_git_block(&git);
        if !lines.is_empty() {
            push_block(&mut blocks, "git", 4, &lines.join("\n"));
        }
    }

    let adrs = load_adrs(&project_root);
    if !adrs.is_empty() {
        let lines = format_adrs_block(&adrs);
        if !lines.is_empty() {
            push_block(&mut blocks, "adrs", 5, &lines.join("\n"));
        }
    }

    let sessions_all = read_jsonl(&sess_file).unwrap_or_default();
    let sess_lines = format_sessions_block(&sessions_all);
    if !sess_lines.is_empty() {
        push_block(&mut blocks, "sessions", 7, &sess_lines.join("\n"));
    }

    let corrections_all = read_jsonl(&corrections_file).unwrap_or_default();
    let corr_lines = format_corrections_block(&corrections_all);
    if !corr_lines.is_empty() {
        push_block(&mut blocks, "corrections", 7, &corr_lines.join("\n"));
    }

    let decisions_all = read_jsonl(&decisions_file).unwrap_or_default();
    let dec_lines = format_decisions_log_block(&decisions_all);
    if !dec_lines.is_empty() {
        push_block(&mut blocks, "decisions_log", 8, &dec_lines.join("\n"));
    }

    let gotchas_all = read_jsonl(&gotchas_file).unwrap_or_default();
    let gotch_lines = format_gotchas_log_block(&gotchas_all);
    if !gotch_lines.is_empty() {
        push_block(&mut blocks, "gotchas_log", 9, &gotch_lines.join("\n"));
    }

    let observations_all = read_jsonl(&obs_file).unwrap_or_default();
    let obs_lines = format_observations_block(&observations_all, session_id.as_deref());
    if !obs_lines.is_empty() {
        push_block(&mut blocks, "observations", 10, &obs_lines.join("\n"));
    }

    let idx_lines = indexer_context_lines(&md, 25);
    if !idx_lines.is_empty() {
        push_block(&mut blocks, "code_index", 6, &idx_lines.join("\n"));
    }

    let graph = load_graph(&md, None);
    let now_ms = now_ms();
    let graph_lines = graph_context_lines(&graph, now_ms);
    if !graph_lines.is_empty() {
        let mut full = vec!["## Padrões detectados".to_string()];
        full.extend(graph_lines);
        push_block(&mut blocks, "graph", 11, &full.join("\n"));
    }

    // 7. Token budget.
    let budget = std::env::var("POUPAR_TOKEN_BUDGET")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(4000);

    let (included, skipped, total_tokens) = apply_token_budget(blocks, budget);

    // 8. Persiste telemetria.
    let stats = load_stats(&md).unwrap_or_else(|| json!({"sessions": 0, "observations": 0}));
    let mut stats_obj = stats.as_object().cloned().unwrap_or_default();
    stats_obj.insert("last_injection_tokens".into(), json!(total_tokens));
    stats_obj.insert("last_injection_budget".into(), json!(budget));
    stats_obj.insert("last_injection_at".into(), json!(ts.clone()));
    stats_obj.insert(
        "last_injection_blocks".into(),
        json!(included
            .iter()
            .map(|b| format!("{}:{}", b.name, b.tokens))
            .collect::<Vec<_>>()),
    );
    stats_obj.insert(
        "last_injection_skipped".into(),
        json!(skipped
            .iter()
            .map(|b| format!("{}:{}", b.name, b.tokens))
            .collect::<Vec<_>>()),
    );
    save_stats(&md, &Value::Object(stats_obj));

    // 9. Renderiza payload final.
    let context = render_final_context(&included, &skipped, total_tokens, budget);
    if context.is_empty() {
        println!("{}", render_hook_payload(None));
    } else {
        println!("{}", render_hook_payload(Some(&context)));
    }

    ExitCode::SUCCESS
}

fn push_block(blocks: &mut Vec<Block>, name: &str, priority: i64, text: &str) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    let tokens = estimate_tokens(trimmed);
    blocks.push(Block {
        name: name.to_string(),
        priority,
        text: trimmed.to_string(),
        tokens,
    });
}

fn read_to_trimmed(path: &Path) -> String {
    if !path.exists() {
        return String::new();
    }
    fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn read_git_context(root: &Path) -> Option<GitContext> {
    let branch = run_git(root, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    let branch = branch.trim().to_string();
    if branch.is_empty() {
        return None;
    }

    let status_raw = run_git(root, &["status", "--porcelain"]).unwrap_or_default();
    let modified: Vec<String> = status_raw
        .split('\n')
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let cut = l.chars().take(3).map(|c| c.len_utf8()).sum::<usize>();
            l.get(cut..).unwrap_or("").trim().to_string()
        })
        .filter(|s| !s.is_empty())
        .collect();

    let log_raw = run_git(root, &["log", "-5", "--pretty=%h|%s"]).unwrap_or_default();
    let recent_commits: Vec<RecentCommit> = log_raw
        .split('\n')
        .filter(|l| l.contains('|'))
        .map(|l| {
            let (sha, rest) = l.split_once('|').unwrap();
            let msg: String = rest.chars().take(80).collect();
            RecentCommit {
                sha: sha.to_string(),
                msg,
            }
        })
        .collect();

    Some(GitContext {
        branch,
        modified,
        recent_commits,
    })
}

fn run_git(cwd: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

fn load_adrs(root: &Path) -> Vec<AdrEntry> {
    let candidates = ["docs/adr", "docs/adrs", "docs/decisions", "adr"];
    for c in candidates {
        let dir = root.join(c);
        if !dir.exists() {
            continue;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut files: Vec<String> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.ends_with(".md") {
                    Some(name)
                } else {
                    None
                }
            })
            .collect();
        files.sort();
        let take_from = files.len().saturating_sub(8);
        let mut out = Vec::new();
        for f in files[take_from..].iter() {
            let full = dir.join(f);
            let content = fs::read_to_string(&full).unwrap_or_default();
            let title = adr_title_from_content(f, &content);
            out.push(AdrEntry {
                file: format!("{}/{}", c, f),
                title,
            });
        }
        return out;
    }
    Vec::new()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn now_iso() -> String {
    format_iso_millis(now_ms())
}
