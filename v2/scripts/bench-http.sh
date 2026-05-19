#!/usr/bin/env bash
# Bench HTTP do pmem-http: TCP vs Unix socket.
#
# Endpoint testado: GET /api/auth/server-token (leve, sem dependência
# de vault unlocked, retorna JSON pequeno). Bate N requests em loop
# sequencial + medição de tempo total → RPS.
#
# Não é wrk/ab (não temos), então é benchmark grosso. Para comparação
# relativa entre TCP e Unix é suficiente; pra números absolutos use wrk.
#
# Uso: ./bench-http.sh [N_REQUESTS]

set -euo pipefail
export LC_NUMERIC=C  # awk/printf usam ponto decimal, não vírgula (locale-safe)

N="${1:-2000}"
PORT="${PORT:-37999}"
TMP="$(mktemp -d)"
SOCK="$TMP/pmem.sock"
BIN="${BIN:-$(pwd)/v2/target/release/pmem-http}"

if [[ ! -x "$BIN" ]]; then
  echo "binário não encontrado: $BIN"
  echo "rode: cargo build --release --manifest-path v2/Cargo.toml --bin pmem-http"
  exit 1
fi

cleanup() {
  pkill -P $$ pmem-http 2>/dev/null || true
  rm -rf "$TMP"
}
trap cleanup EXIT

run_loop() {
  local url="$1"
  local opts="$2"
  local n="$3"
  local start=$(date +%s.%N)
  for ((i=0; i<n; i++)); do
    # shellcheck disable=SC2086
    curl -s $opts "$url" > /dev/null
  done
  local end=$(date +%s.%N)
  local elapsed
  elapsed=$(awk "BEGIN { print $end - $start }")
  local rps
  rps=$(awk "BEGIN { print $n / $elapsed }")
  local per_min
  per_min=$(awk "BEGIN { printf \"%.0f\", $rps * 60 }")
  printf "  %d reqs em %.2fs → %.0f req/s (%.0f req/min)\n" "$n" "$elapsed" "$rps" "$per_min"
}

echo "== bench HTTP pmem-http =="
echo "N=$N requests por modo"
echo "binário: $BIN"
echo ""

# ── TCP ────────────────────────────────────────────────────────────────
echo "[TCP] subindo em 127.0.0.1:$PORT ..."
PROJECT_MEM_PORT="$PORT" CLAUDE_CWD="$TMP" "$BIN" 2>"$TMP/tcp.log" &
TCP_PID=$!
# Espera bind (pmem-http imprime "escutando em" pra stderr).
for _ in $(seq 1 20); do
  if grep -q "escutando em" "$TMP/tcp.log" 2>/dev/null; then break; fi
  sleep 0.1
done
if ! curl -s -o /dev/null --max-time 1 "http://127.0.0.1:$PORT/" 2>/dev/null; then
  echo "TCP server não respondeu — log:"
  cat "$TMP/tcp.log"
  exit 1
fi
echo "TCP: warmup..."
run_loop "http://127.0.0.1:$PORT/api/auth/server-token" "" 100
echo "TCP: bench:"
run_loop "http://127.0.0.1:$PORT/api/auth/server-token" "" "$N"
kill "$TCP_PID" 2>/dev/null || true
wait "$TCP_PID" 2>/dev/null || true
sleep 0.3

# ── Unix socket ────────────────────────────────────────────────────────
echo ""
echo "[Unix] subindo em $SOCK ..."
PROJECT_MEM_BIND_UNIX="$SOCK" CLAUDE_CWD="$TMP" "$BIN" 2>"$TMP/unix.log" &
UNIX_PID=$!
for _ in $(seq 1 20); do
  if [[ -S "$SOCK" ]]; then break; fi
  sleep 0.1
done
if ! curl -s -o /dev/null --max-time 1 --unix-socket "$SOCK" "http://localhost/" 2>/dev/null; then
  echo "Unix server não respondeu — log:"
  cat "$TMP/unix.log"
  exit 1
fi
echo "Unix: warmup..."
run_loop "http://localhost/api/auth/server-token" "--unix-socket $SOCK" 100
echo "Unix: bench:"
run_loop "http://localhost/api/auth/server-token" "--unix-socket $SOCK" "$N"
kill "$UNIX_PID" 2>/dev/null || true
wait "$UNIX_PID" 2>/dev/null || true

echo ""
echo "== fim =="
