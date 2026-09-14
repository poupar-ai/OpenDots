#!/usr/bin/env bash
# Bench end-to-end de POST /api/agents/handoff_batch. Compara contra
# bench-handoff.sh (single). Espera-se ~5-10× mais throughput total
# quando cliente pode batchear.
#
# Uso: ./bench-handoff-batch.sh [N_BATCHES] [BATCH_SIZE]
#   default: 50 batches × 20 items = 1000 handoffs totais.

set -euo pipefail
export LC_NUMERIC=C

N_BATCHES="${1:-50}"
BATCH_SIZE="${2:-20}"
TOTAL=$((N_BATCHES * BATCH_SIZE))
PORT="${PORT:-37985}"
TMP="$(mktemp -d)"
BIN="${BIN:-$(pwd)/v2/target/release/poupar-http}"

if [[ ! -x "$BIN" ]]; then
  echo "binário não encontrado: $BIN"
  exit 1
fi

cleanup() {
  pkill -P $$ poupar-http 2>/dev/null || true
  rm -rf "$TMP"
}
trap cleanup EXIT

URL="http://127.0.0.1:$PORT"

echo "== bench handoff BATCH =="
echo "N_BATCHES=$N_BATCHES, BATCH_SIZE=$BATCH_SIZE → TOTAL=$TOTAL handoffs"
echo ""

POUPAR_DEV_MODE=1 POUPAR_PORT="$PORT" CLAUDE_CWD="$TMP" "$BIN" 2>"$TMP/server.log" &
for _ in $(seq 1 30); do
  curl -s -o /dev/null --max-time 1 "$URL/" 2>/dev/null && break
  sleep 0.1
done

curl -s -X POST "$URL/api/vault/init" -H 'Content-Type: application/json' -d '{"password":"benchpass123"}' > /dev/null
curl -s -X POST "$URL/api/vault/unlock" -H 'Content-Type: application/json' -d '{"password":"benchpass123"}' > /dev/null
curl -s -X POST "$URL/api/agents" -H 'Content-Type: application/json' \
  -d '{"name":"manager","type":"core","model":"sonnet-4.6","role":"x","tools":[],"can_talk_to":["oracle"]}' > /dev/null
curl -s -X POST "$URL/api/agents" -H 'Content-Type: application/json' \
  -d '{"name":"oracle","type":"core","model":"sonnet-4.6","role":"x","tools":[],"can_talk_to":["manager"]}' > /dev/null

TOKEN=$(curl -s -X POST "$URL/api/_dev/mint_token" -H 'Content-Type: application/json' \
  -d '{"agent":"manager"}' | grep -oP '"token":"\K[^"]+')

# Monta body de batch (mesmo body reutilizado em todas as requests).
ITEMS=""
for ((i=0; i<BATCH_SIZE; i++)); do
  ITEMS="${ITEMS}{\"to\":\"oracle\",\"message\":\"batch item $i\"},"
done
ITEMS="${ITEMS%,}"
BODY="{\"items\":[$ITEMS]}"

# Warmup.
for _ in $(seq 1 5); do
  curl -s -o /dev/null -X POST "$URL/api/agents/handoff_batch" \
    -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
    -d "$BODY"
done

echo "bench:"
start=$(date +%s.%N)
for ((i=0; i<N_BATCHES; i++)); do
  curl -s -o /dev/null -X POST "$URL/api/agents/handoff_batch" \
    -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
    -d "$BODY"
done
end=$(date +%s.%N)
elapsed=$(awk "BEGIN { print $end - $start }")
batch_rps=$(awk "BEGIN { print $N_BATCHES / $elapsed }")
handoff_rps=$(awk "BEGIN { print $TOTAL / $elapsed }")
handoff_per_min=$(awk "BEGIN { printf \"%.0f\", $handoff_rps * 60 }")
printf "  %d batches × %d items em %.2fs\n" "$N_BATCHES" "$BATCH_SIZE" "$elapsed"
printf "  → %.0f batch/s, %.0f handoffs/s (%.0f handoffs/min)\n" "$batch_rps" "$handoff_rps" "$handoff_per_min"
echo ""
echo "comparar com bench-handoff.sh (single): ~140 handoffs/s (~8.5k/min)"
echo "speedup esperado: ~$(awk "BEGIN { printf \"%.0fx\", $handoff_rps / 140 }")"
