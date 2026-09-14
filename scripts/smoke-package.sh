#!/usr/bin/env bash
# smoke-package.sh — extrai o tarball de release e verifica que ele
# funciona fora do clone.
#
# É o teste que pega a classe de bug mais fácil de deixar passar aqui:
# binário que só acha `plugin/` e a UI porque o `cwd` por acaso era o
# repositório. No tarball não há repositório nenhum — a resolução tem
# que sair do próprio executável (`core_paths::install`).
#
# Roda depois de `scripts/package.sh`. Sem argumentos, pega o tarball do
# target nativo em `dist/`.
#
# uso: scripts/smoke-package.sh [caminho/do/tarball.tar.gz]

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

TARBALL="${1:-}"
if [ -z "$TARBALL" ]; then
  target="$(cd v2 && rustc -vV | sed -n 's/^host: //p')"
  version="$(sed -n 's/^version *= *"\(.*\)"/\1/p' v2/Cargo.toml | head -1)"
  TARBALL="dist/open-poupar-${version}-${target}.tar.gz"
fi
[ -f "$TARBALL" ] || { echo "smoke: tarball não existe: $TARBALL" >&2; exit 1; }

PORT="${POUPAR_SMOKE_PORT:-38571}"
WORK="$(mktemp -d)"
HTTP_PID=""

cleanup() {
  if [ -n "$HTTP_PID" ]; then
    kill "$HTTP_PID" 2>/dev/null || true
  fi
  rm -rf "$WORK"
}
trap cleanup EXIT

fail() { echo "✗ $*" >&2; exit 1; }
pass() { echo "✓ $*"; }

echo "==> extraindo $TARBALL"
tar -xzf "$TARBALL" -C "$WORK"
ROOT="$(find "$WORK" -maxdepth 1 -mindepth 1 -type d | head -1)"
[ -n "$ROOT" ] || fail "tarball vazio"

# Projeto de teste bem longe do clone — se algum path ainda for relativo
# ao repositório, quebra aqui.
PROJ="$WORK/projeto"
mkdir -p "$PROJ"
git -C "$PROJ" init -q

# ─── hooks ───────────────────────────────────────────────────────────
#
# O dispatcher tem que escolher o binário (e não o `.cjs`), e a
# observação tem que cair no `.poupar` do PROJETO.

resolved="$(CLAUDE_PLUGIN_ROOT="$ROOT/plugin" bash "$ROOT/plugin/bin-path.sh" --installed poupar-hook-observe)" \
  || fail "bin-path.sh não achou poupar-hook-observe na instalação"
[ "$resolved" = "$ROOT/plugin/bin/poupar-hook-observe" ] \
  || fail "bin-path.sh resolveu pro lugar errado: $resolved"
pass "bin-path.sh resolve o binário instalado"

printf '{"session_id":"smoke","tool_name":"Bash","tool_input":{"command":"pytest -q"},"tool_response":{"exit_code":0,"stdout":"3 passed"},"cwd":"%s"}' "$PROJ" \
  | CLAUDE_PLUGIN_ROOT="$ROOT/plugin" bash "$ROOT/plugin/hooks/run-hook.sh" observe \
  || fail "hook observe saiu com erro"
grep -q '"cmd":"pytest -q"' "$PROJ/.poupar/observations.jsonl" \
  || fail "observação não foi gravada no projeto"
pass "hook observe grava no .poupar do projeto"

# ─── MCP ─────────────────────────────────────────────────────────────

printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}\n' \
  | "$ROOT/plugin/bin/poupar-mcp" \
  | grep -q '"protocolVersion"' \
  || fail "poupar-mcp não respondeu ao handshake"
pass "poupar-mcp responde ao handshake"

# ─── CLI ─────────────────────────────────────────────────────────────
#
# Numa instalação com binário, o MCP tem que ser registrado nativo —
# se cair no Node, o usuário sem Node fica sem MCP e nada avisa.

CLAUDE_CONFIG_DIR="$WORK/claude" "$ROOT/plugin/bin/poupar" install "$ROOT" >/dev/null \
  || fail "poupar install falhou"
grep -q '"command": ".*/plugin/bin/poupar-mcp"' "$WORK/claude/settings.json" \
  || fail "poupar install registrou o MCP via Node, não o binário nativo"
pass "poupar install registra o MCP nativo"

# ─── HTTP + UI ───────────────────────────────────────────────────────

# Porta ocupada por um processo alheio é armadilha silenciosa: os
# `curl` abaixo respondem, mas quem responde é o outro servidor — e o
# smoke "passa" ou falha por motivo errado. Aborta antes de começar.
if curl -sf --max-time 2 "http://127.0.0.1:$PORT/api/data" >/dev/null 2>&1; then
  fail "porta $PORT já está ocupada por outro poupar-http. Libere ou use POUPAR_SMOKE_PORT."
fi

echo "==> subindo poupar-http na porta $PORT"
# `exec` para que $! seja o PID do servidor, e não o do subshell — sem
# isso o `kill` do cleanup mata a casca e deixa o servidor órfão
# segurando a porta pro próximo run.
( cd "$PROJ" && exec env POUPAR_PORT="$PORT" "$ROOT/plugin/bin/poupar-http" >"$WORK/http.log" 2>&1 ) &
HTTP_PID=$!

for _ in $(seq 1 60); do
  curl -sf "http://127.0.0.1:$PORT/api/data" >/dev/null 2>&1 && break
  # Se o processo já morreu, esperar os 15s completos não ajuda.
  kill -0 "$HTTP_PID" 2>/dev/null || break
  sleep 0.25
done
curl -sf "http://127.0.0.1:$PORT/api/data" >/dev/null 2>&1 \
  || { cat "$WORK/http.log" >&2; fail "poupar-http não subiu"; }
pass "poupar-http subiu"

index="$(curl -fsS "http://127.0.0.1:$PORT/")"
# A página de fallback ("rode o build") cita ui-next/dist. Se ela apareceu,
# o binário não achou a UI que veio no tarball.
case "$index" in
  *ui-next/dist*) fail "servindo a página de fallback — a UI do tarball não foi encontrada" ;;
esac
case "$index" in
  *"/assets/"*) ;;
  *) fail "index.html servido não referencia /assets/" ;;
esac
pass "UI do tarball é servida"

# `-print -quit` em vez de `| head -1`: evita o SIGPIPE no find.
css="$(basename "$(find "$ROOT/ui/assets" -name '*.css' -print -quit)")"
code="$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$PORT/assets/$css")"
[ "$code" = "200" ] || fail "asset /assets/$css devolveu $code"
pass "assets estáticos são servidos"

curl -fsS "http://127.0.0.1:$PORT/api/seeds" | grep -q '"categories"' \
  || fail "/api/seeds vazio — plugin_dir não foi resolvido"
pass "plugin/ é resolvido a partir do executável"

echo
echo "✓ smoke-package OK ($(basename "$TARBALL"))"
