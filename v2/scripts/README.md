# v2/scripts — utilitários de benchmark

Scripts pra medir performance do sistema em isolamento ou end-to-end.
Não rodam automaticamente — execute manualmente quando quiser baseline,
ou quando suspeitar de regressão.

## Pré-requisitos

```bash
cargo build --release --manifest-path v2/Cargo.toml --bin pmem-http
```

(Os scripts de HTTP precisam do binário release pra ter números reais.
Debug é ~10× mais lento.)

## Benchmarks disponíveis

### 1. `bench-storage` — throughput do JsonlRepository (puro)

Mede o teto teórico do storage layer (sem HTTP, sem agentes). Isolado.

```bash
cargo run --release --manifest-path v2/Cargo.toml -p core-storage --bin bench
```

**Mede**:
- `append` single, com e sem fsync
- `append_batch` com vários tamanhos
- Overhead da rotação automática
- `read_all` e `tail(n)`

**Quando rodar**:
- Antes/depois de mexer no `core-storage` (regressão de performance).
- Pra capacity planning: "estamos longe do teto?"

**Números típicos** (SSD local, single thread, no contention):
| operação | throughput |
|---|---|
| append + fsync OFF | ~110k ops/s (~6.7M/min) |
| append + fsync ON  | ~420 ops/s (~25k/min, disk-bound) |
| batch×1000 + fsync OFF | ~3M ops/s (~180M/min) |
| batch×1000 + fsync ON  | ~225k ops/s (~13.5M/min) |
| read_all (100k items) | ~1.6M items/s |

### 2. `bench-http.sh` — TCP vs Unix socket (overhead de transport)

Compara latência/throughput entre TCP localhost e Unix socket pra um
endpoint leve (`GET /api/auth/server-token`).

```bash
v2/scripts/bench-http.sh [N_REQUESTS]   # default 2000
```

**Quando rodar**:
- Antes de decidir migrar pra Unix-only.
- Confirmar que mudança no listener não regrediu RPS.

**Números típicos**:
| transport | RPS |
|---|---|
| TCP localhost | ~142 req/s (~8.5k/min) |
| Unix socket   | ~146 req/s (~8.7k/min) |

Diferença é margem de ruído — Unix vale pelo **isolamento** (não LAN),
não throughput.

### 3. `bench-handoff.sh` — handoff end-to-end (JWT + queue + storage)

Mede o caminho completo de POST `/api/agents/handoff`: auth middleware,
JWT verify, manifest lookup, `can_talk_to` check, destructive scan,
`append_chat` × 1, `append_agent_log` × 2, `bus.emit`, `auto_queue.enqueue`.

```bash
v2/scripts/bench-handoff.sh [N_REQUESTS]   # default 500
```

**Setup automático**: cria vault, agentes manager+oracle (com keypairs),
mintea JWT via endpoint dev (env `PROJECT_MEM_DEV_MODE=1`).

**Quando rodar**:
- Validar throughput do caminho mais crítico.
- Depois de mexer em qualquer parte do handoff (auth, vault, queue).

**Números típicos**: ~140 req/s (~8.4k/min). Gargalo é HTTP+curl-spawn,
não a lógica. Pra subir, usar HTTP/2 + connection pool no cliente, ou
adicionar endpoint batch (`POST /api/agents/handoff_batch`).

## Convenções

- **Não commitar resultados**: scripts imprimem, mas não persistem.
  Salve em local seu (PR description, gist, etc.) se quiser comparar
  histórico.
- **Não rodar em paralelo**: cada script spawna pmem-http próprio.
  Múltiplos em paralelo competem por CPU e tornam números ruidosos.
- **Mate zombies entre runs**: se um script crashar, `pkill pmem-http`
  antes de rodar de novo. Senão ficam instances vivas competindo por
  porta.

## Notas de limitação

- Single-thread, sem contention. Em multi-thread com lock contention no
  `Mutex` do JsonlRepository, throughput cai (não medido).
- `curl` em loop é mais lento que `wrk`/`ab` pelo overhead de spawn.
  Pra medir teto real do servidor, instalar `wrk` (não disponível por
  default neste ambiente).
- HTTP bench usa endpoint leve (`/api/auth/server-token`). Endpoints
  com lógica pesada (handoff) têm RPS menor — vide `bench-handoff.sh`.
