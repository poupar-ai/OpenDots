#!/usr/bin/env bash
set -u

# SessionEnd hook do project-mem: dispara memory-save só na sessão REPL principal.
# Pula qualquer sub-agente spawnado pelo server do project-mem e qualquer recursão.

[ -n "${CLAUDE_PROJECT_MEM_SUBAGENT:-}" ] && exit 0
[ -n "${CLAUDE_HOOK_MEMORY_SAVE_RUNNING:-}" ] && exit 0

# Requer o comando per-project .claude/commands/memory-save.md na raiz do projeto.
PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$PWD}"
[ -f "$PROJECT_DIR/.claude/commands/memory-save.md" ] || exit 0

export CLAUDE_HOOK_MEMORY_SAVE_RUNNING=1

PROMPT='A sessão está encerrando. Salve o contexto relevante seguindo as instruções em .claude/commands/memory-save.md. Só escreva em .claude/memory/short-term/. Se não houve nada relevante a salvar nesta sessão, não faça nada.'

claude -p "$PROMPT" --model claude-haiku-4-5-20251001 >/dev/null 2>&1 || true
exit 0
