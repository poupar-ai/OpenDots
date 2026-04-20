//! `pmem-mcp` — binário MCP stdio.
//!
//! Paridade com `plugin/mcp/server.cjs`:
//! - Transport: JSON-RPC 2.0 newline-delimited sobre stdin/stdout.
//! - Tools: `mem_context`, `mem_read_project`, `mem_search`, `mem_add_note`.
//! - cwd = `process.cwd()`, mem_dir = `findProjectRoot(cwd)/.claude-project-memory`.
//! - Linhas não-JSON são silenciosamente descartadas (paridade com
//!   `try { JSON.parse(...) } catch { return }` do `.cjs`).
//! - EOF → exit 0.

use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use api_mcp::{handle_rpc, FsToolContext};
use core_paths::{find_project_root, MEM_DIR_NAME};
use domain_graph::format_iso_millis;
use serde_json::Value;

fn main() -> ExitCode {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let cwd = std::env::var("CLAUDE_CWD")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("PWD").map(PathBuf::from))
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };

        // `mem_dir` recomputado por mensagem para bater com o `.cjs`
        // (que chama `findProjectRoot(process.cwd())` a cada tool call).
        let project_root = find_project_root(&cwd);
        let mem_dir = project_root.join(MEM_DIR_NAME);
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let ctx = FsToolContext {
            mem_dir: &mem_dir,
            now_ms: ms,
            now_iso: format_iso_millis(ms),
        };

        if let Some(resp) = handle_rpc(&msg, &ctx) {
            let _ = writeln!(out, "{}", resp.body);
            let _ = out.flush();
        }
    }

    ExitCode::SUCCESS
}
