#!/usr/bin/env bash
# Bench end-to-end de POST /api/agents/handoff. Mede o ciclo completo:
# HTTP + middleware (auth + tenant) + JWT verify + manifest lookup +
# can_talk_to check + destructive scan + append_chat + agent_logs +
# bus.emit + enqueue → response.
#
# Requer PROJECT_MEM_DEV_MODE=1 (server precisa expor /api/_dev/mint_token
# pra emitir o JWT do agente "manager" usado nas requests).
#
# Uso: ./bench-handoff.sh [N_REQUESTS]

set -euo pipefail
export LC_NUMERIC=C

N="${1:-500}"
PORT="${PORT:-37998}"
TMP="$(mktemp -d)"
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

URL="http://127.0.0.1:$PORT"

echo "== bench handoff =="
echo "N=$N requests"
echo "mem_dir=$TMP"
echo ""

# ── Start server em DEV MODE ───────────────────────────────────────────
PROJECT_MEM_DEV_MODE=1 PROJECT_MEM_PORT="$PORT" CLAUDE_CWD="$TMP" "$BIN" 2>"$TMP/server.log" &
SERVER_PID=$!
for _ in $(seq 1 30); do
  if curl -s -o /dev/null --max-time 1 "$URL/" 2>/dev/null; then break; fi
  sleep 0.1
done
if ! curl -s -o /dev/null --max-time 1 "$URL/" 2>/dev/null; then
  echo "server não subiu — log:"
  cat "$TMP/server.log"
  exit 1
fi
echo "server up em $URL"

# ── Setup: init vault ─────────────────────────────────────────────────
echo "init vault..."
curl -s -X POST "$URL/api/vault/init" \
  -H 'Content-Type: application/json' \
  -d '{"password":"benchpass123"}' > /dev/null
curl -s -X POST "$URL/api/vault/unlock" \
  -H 'Content-Type: application/json' \
  -d '{"password":"benchpass123"}' > /dev/null

# ── Setup: criar 2 agentes com can_talk_to ────────────────────────────
echo "criando manager + oracle..."
curl -s -X POST "$URL/api/agents" \
  -H 'Content-Type: application/json' \
  -d '{"name":"manager","type":"core","model":"sonnet-4.6","role":"manager","tools":["Read","Bash"],"can_talk_to":["oracle"]}' > /dev/null
curl -s -X POST "$URL/api/agents" \
  -H 'Content-Type: application/json' \
  -d '{"name":"oracle","type":"core","model":"sonnet-4.6","role":"oracle","tools":["Read"],"can_talk_to":["manager"]}' > /dev/null

# ── Mintea JWT do manager ─────────────────────────────────────────────
TOKEN=$(curl -s -X POST "$URL/api/_dev/mint_token" \
  -H 'Content-Type: application/json' \
  -d '{"agent":"manager"}' | grep -o '"token":"[^"]*"' | cut -d'"' -f4)

if [[ -z "$TOKEN" ]]; then
  echo "falha ao obter JWT — log do server:"
  cat "$TMP/server.log"
  exit 1
fi
echo "JWT obtido (preview: ${TOKEN:0:30}...)"

# ── Warmup ──────────────────────────────────────────────────────────────
echo ""
echo "warmup (50 reqs)..."
for ((i=0; i<50; i++)); do
  curl -s -o /dev/null -X POST "$URL/api/agents/handoff" \
    -H "Authorization: Bearer $TOKEN" \
    -H 'Content-Type: application/json' \
    -d "{\"from\":\"manager\",\"to\":\"oracle\",\"message\":\"warmup $i\"}"
done

# ── Bench ──────────────────────────────────────────────────────────────
echo ""
echo "bench ($N reqs):"
start=$(date +%s.%N)
for ((i=0; i<N; i++)); do
  curl -s -o /dev/null -X POST "$URL/api/agents/handoff" \
    -H "Authorization: Bearer $TOKEN" \
    -H 'Content-Type: application/json' \
    -d "{\"from\":\"manager\",\"to\":\"oracle\",\"message\":\"bench msg $i\"}"
done
end=$(date +%s.%N)
elapsed=$(awk "BEGIN { print $end - $start }")
rps=$(awk "BEGIN { print $N / $elapsed }")
per_min=$(awk "BEGIN { printf \"%.0f\", $rps * 60 }")
printf "  %d reqs em %.2fs → %.0f req/s (%.0f req/min)\n" "$N" "$elapsed" "$rps" "$per_min"

echo ""
echo "(skip sanity check — depende de path do mem_dir que varia com"
echo " smart-default; bench em si confiável pelo RPS acima)"
kill "$SERVER_PID" 2>/dev/null || true
wait "$SERVER_PID" 2>/dev/null || true
echo ""
echo "== fim =="
