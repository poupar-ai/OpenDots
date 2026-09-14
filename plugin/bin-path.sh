#!/usr/bin/env bash
# bin-path.sh — imprime o caminho de um binário compilado do project-mem.
#
# Fonte única de verdade pra "onde os binários moram", porque isso muda
# conforme o layout da instalação:
#
#   tarball (curl | bash)   <root>/plugin/bin/<nome>
#   clone git (make)        <repo>/v2/target/release/<nome>
#   override                $PROJECT_MEM_BIN_DIR/<nome>
#
# uso:
#   bin-path.sh <nome>              # procura nos três, dev build incluído
#   bin-path.sh --installed <nome>  # só binários instalados (sem dev build)
#
# `--installed` é o que os hooks usam: no clone git eles devem continuar
# rodando o `.cjs`, não um `v2/target/release/` possivelmente desatualizado
# de um `cargo build` antigo. Quem quiser testar o build local aponta
# `PROJECT_MEM_BIN_DIR` explicitamente.
#
# Sai 0 e imprime o caminho quando acha; sai 1 e não imprime nada quando
# não acha (caller decide o fallback).

set -u

INCLUDE_DEV_BUILD=1
if [ "${1:-}" = "--installed" ]; then
  INCLUDE_DEV_BUILD=0
  shift
fi

NAME="${1:-}"
if [ -z "$NAME" ]; then
  echo "bin-path.sh: falta o nome do binário" >&2
  exit 2
fi

PLUGIN_ROOT="${CLAUDE_PLUGIN_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)}"

candidates=("${PROJECT_MEM_BIN_DIR:-}" "$PLUGIN_ROOT/bin")
if [ "$INCLUDE_DEV_BUILD" = "1" ]; then
  candidates+=("$PLUGIN_ROOT/../v2/target/release")
fi

for dir in "${candidates[@]}"; do
  [ -n "$dir" ] || continue
  if [ -x "$dir/$NAME" ]; then
    (cd "$dir" && printf '%s/%s\n' "$(pwd)" "$NAME")
    exit 0
  fi
done

exit 1
