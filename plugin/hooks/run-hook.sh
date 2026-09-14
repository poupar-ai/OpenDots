#!/usr/bin/env bash
# run-hook.sh — despacha um hook do open-poupar.ai para o binário
# correspondente.
#
# Os hooks do Claude Code apontam pra cá em vez de pro binário direto
# porque o caminho muda conforme o layout da instalação: `plugin/bin/`
# no tarball, `v2/target/release/` no clone. Quem resolve isso é o
# `bin-path.sh`.
#
# uso: run-hook.sh <session-init|observe|session-end|startup>
#
# stdin (JSON do Claude Code) é repassado intacto; stdout/stderr e o
# exit code são os do binário.

set -u

HOOK="${1:-}"
if [ -z "$HOOK" ]; then
  echo "run-hook.sh: falta o nome do hook" >&2
  exit 2
fi

PLUGIN_ROOT="${CLAUDE_PLUGIN_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"

if bin="$(CLAUDE_PLUGIN_ROOT="$PLUGIN_ROOT" bash "$PLUGIN_ROOT/bin-path.sh" "poupar-hook-$HOOK")"; then
  exec "$bin"
fi

echo "run-hook.sh: binário poupar-hook-$HOOK não encontrado a partir de $PLUGIN_ROOT." >&2
echo "  Instalação por clone precisa de 'make build' antes." >&2
exit 127
