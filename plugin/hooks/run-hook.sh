#!/usr/bin/env bash
# run-hook.sh — despacha um hook do project-mem para a implementação
# disponível na instalação corrente.
#
# Existem dois layouts oficiais e eles rodam runtimes diferentes:
#
#   tarball (curl | bash)     clone git (make install)
#   <root>/plugin/bin/          <repo>/plugin/scripts/*.cjs
#     pmem-hook-<nome>            rodados por Node
#
# O binário compilado vence quando está instalado — só existe no tarball,
# então a instalação por clone continua no caminho `.cjs` de sempre, sem
# mudança de comportamento. Pra testar o build local de dentro do clone,
# aponte `PROJECT_MEM_BIN_DIR=v2/target/release`.
#
# uso: run-hook.sh <session-init|observe|session-end|startup>
#
# stdin (JSON do Claude Code) é repassado intacto; stdout/stderr e o
# exit code são os da implementação escolhida.

set -u

HOOK="${1:-}"
if [ -z "$HOOK" ]; then
  echo "run-hook.sh: falta o nome do hook" >&2
  exit 2
fi

PLUGIN_ROOT="${CLAUDE_PLUGIN_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"

# 1. Binário compilado (`--installed`: sem dev build — ver bin-path.sh).
if bin="$(CLAUDE_PLUGIN_ROOT="$PLUGIN_ROOT" bash "$PLUGIN_ROOT/bin-path.sh" --installed "pmem-hook-$HOOK")"; then
  exec "$bin"
fi

# 2. Fallback Node. Prefere a versão mais nova sob o nvm — a instalação
#    padrão do macOS/Linux deixa `node` fora do PATH de processos não
#    interativos, e o hook roda sem shell de login.
script="$PLUGIN_ROOT/scripts/$HOOK.cjs"
if [ -f "$script" ]; then
  NODE_BIN="$(ls "$HOME"/.nvm/versions/node/*/bin/node 2>/dev/null | sort -V | tail -1)"
  [ -z "$NODE_BIN" ] && NODE_BIN="node"
  exec "$NODE_BIN" "$script"
fi

echo "run-hook.sh: nenhuma implementação para o hook '$HOOK' em $PLUGIN_ROOT" >&2
exit 127
