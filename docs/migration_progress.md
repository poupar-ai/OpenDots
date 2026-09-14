# Migration Progress — open-poupar Node→Rust

> Rastreio vivo do port `plugin/scripts/*.cjs` → `v2/crates/domain-*`.
> Atualizado a cada sub-etapa. Sobrevive a restarts da máquina.
>
> Plano macro: [`migrato_to_rust.md`](./migrato_to_rust.md)
> Achados: [`migration_findings.md`](./migration_findings.md)

## Convenções

- **Status:** ⏳ pendente · 🔄 em curso · ✅ pronto · ❌ bloqueado
- **Sub-etapas:** scaffold (Cargo.toml) → port (src/lib.rs) → unit tests → parity tests → clippy + fmt → commit
- Linha "Achados" cita IDs registrados em `migration_findings.md` (ex.: `F12-01`, `F12-02`).

---

## Fase 2 — 17 domínios (concluída 2026-04-19)

| # | Crate | Origem `.cjs` | Linhas | Status |
|---|---|---|---|---|
| #47 | `domain-vault` | `vault.cjs` | 644 | ✅ |
| #48 | `domain-agents` | `agents.cjs` | 860 | ✅ |
| #49 | `domain-graph` | `graph.cjs` | 583 | ✅ |
| #50 | `domain-indexer` | `indexer.cjs` | 507 | ✅ |
| #51 | `domain-pipelines` | `pipelines.cjs` | 453 | ✅ |
| #52 | `domain-bus` | `bus.cjs` + `router.cjs` | 391+137 | ✅ |
| #53 | `domain-sigilo` | `sigilo.cjs` | 174 | ✅ |
| #54 | `domain-integrity` | `integrity.cjs` | 136 | ✅ |
| #55 | `domain-destructive` | `destructive.cjs` | 255 | ✅ |
| #56 | `domain-best-practices` | `best-practices.cjs` | 253 | ✅ |
| #57a | `domain-firefighter` | `firefighter.cjs` | 305 | ✅ |
| #57b | `domain-nudge` | `nudge.cjs` | 287 | ✅ |
| #58a | `domain-architect` | `architect.cjs` | 171 | ✅ |
| #58b | `domain-ide-adapters` | `ide-adapters.cjs` | 185 | ✅ |
| #58c | `domain-seed` | `seed.cjs` | 380 | ✅ |
| #58d | `domain-skills-install` | `skills-install.cjs` | 125 | ✅ |
| #58e | `domain-session` | `session-init.cjs` + `session-end.cjs` | 411+214 | ✅ |

---

## Fase 3 — adapters outbound com equivalente `.cjs` (em curso)

| # | Crate | Depende de | Cobre | Status |
|---|---|---|---|---|
| F3-01 | `adapter-vault-file` | `domain-vault`, `core-ports::VaultStore` | disk I/O do cofre, device-key cache, UI session, migração PMV1→PMV2, bundle PMVEXP1 | ✅ |
| F3-02 | `adapter-fs` | `core-ports::{ObservationLog,GraphStore,SessionStore}` | JSONL append-only, graph.json, session store | ✅ |

**Adapters v2-only** (sem equivalente `.cjs` — pós-cutover, não são
parte do escopo atual):

- `adapter-firebase` (LicenseValidator)
- `adapter-telemetry`, `adapter-update`, `adapter-notifier`

Esses virão só depois do binário Rust estar em paridade total com o
`.cjs` e o modelo comercial começar. Por ora, NÃO bloqueiam cutover.

---

## Fase 4 — portar o que resta do `.cjs` (~5.192 linhas)

Decisão do dono (2026-04-19): focar só em código que já funciona hoje.
Nada novo até terminar paridade do existente.

| # | Origem `.cjs` | Linhas | Destino Rust | Ordem | Status |
|---|---|---|---|---|---|
| F4-01 | `scripts/install.cjs` + `uninstall.cjs` + `status.cjs` | 160 | `api-cli` (`poupar install`/`uninstall`/`status`) | 1 | ✅ |
| F4-02 | `plugin/scripts/queue.cjs` | 149 | `core-queue` | 2 | ✅ |
| F4-03 | `plugin/scripts/{session-init,session-end,observe,startup}.cjs` | 933 | `api-hooks` (4 binários thin: leem stdin JSON, chamam domain-session + adapters, escrevem efeitos) | 3 | ✅ |
| F4-04 | `plugin/mcp/server.cjs` | 285 | `api-mcp` (stdio, tools `mem.*`) | 4 | ✅ |
| F4-05 | `plugin/ui/server.cjs` | 3.665 | `api-http` (Axum + Tower + SSE + UI via `include_str!`) | 5 | 🔄 |

**F4-05 dividida em sub-fases** (arquivo grande demais pra um passo só):

| Sub | Escopo | Status |
|---|---|---|
| F4-05a | scaffold + static + `/api/network-config` + `/api/bus/*` (state/trace/config/compact) | ✅ |
| F4-05b | Vault endpoints (20 rotas `/api/vault/*`) + guards (lockout, rate-limit) | ✅ |
| F4-05c | Destructive + seeds + best-practices + pipelines + architect + skills + ides | ✅ |
| F4-05d-1 | Humans CRUD + heartbeat (6 rotas `/api/humans/*`) | ✅ |
| F4-05d-2 | Human chat read-only (`/api/humans/:id/chats`, `/api/human-chat/history`) | ✅ |
| F4-05e | Agents CRUD + per-agent PATCH (model, autonomy, scope, vault, context, mcps, doc) | ✅ |
| F4-05f | Chat: `/api/chat/*` (send, history, stream SSE, improve) + spawn Claude + queue drain | ⏳ |
| F4-05g | Wiring: main, bus onRetry/onTimeout, scheduleNudge, runCompaction cron | ⏳ |

**Ordem justificada:** menor → maior, isolado → acoplado. `api-cli`
fecha rápido e valida padrão `api-*`. `core-queue` é atômico e dep
futura do `api-http`. `api-hooks` é onde o `.cjs` efetivamente roda
no runtime do Claude Code — primeira etapa real de cutover. `api-mcp`
e `api-http` são os grandes.

---

## Item em curso

_`api-http` (F4-05) em andamento — F4-05a + F4-05b + F4-05c + F4-05d-1
+ F4-05d-2 + F4-05e prontas.
Próxima: F4-05f (chat SSE + spawn Claude + queue drain + handoff)._

---

## Histórico recente

### ✅ `api-http` F4-05e (agents CRUD + per-agent endpoints) — 2026-04-19
- **Tests verdes**: 32 parity tests (cross-language contra `node
  server.cjs`) + 3 unit tests (`is_valid_doc_name`, `sanitize_string_array`).
- **12 rotas** portadas de `plugin/ui/server.cjs`:
  - `POST   /api/agents`                    — cria agente (ok 200)
  - `DELETE /api/agents/:name`              — 404 unknown, 405 `handoff`,
    400 manager/oracle
  - `PATCH  /api/agents/:name/model`        — model + propagate_seed
    (origin via `_meta.agent_origin`)
  - `PATCH  /api/agents/:name/autonomy`     — {level, call_human_when}
    + propagate_seed
  - `PUT    /api/agents/:name/scope`        — tools + scope.bash_allow
    + audit log `scope_updated`
  - `PUT    /api/agents/:name/vault`        — secrets (ACL) + audit log
    `vault_access_updated`
  - `PUT    /api/agents/:name/context`      — `.md` mini-contexto
  - `PUT    /api/agents/:name/mcps`         — sanitize + save manifest
  - `GET    /api/mcps`                      — global+project+referenced+all
    (dedup BTreeSet + ordem preservada)
  - `PUT    /api/docs/:doc`                 — allowlist SOUL/PROJECT/…
  - `GET    /api/agents/log?agent=X`        — tail agent_logs.jsonl
  - `GET    /api/agents/work?agent=X`       — observations.jsonl filtrado,
    slice last N, reversed
- **Arquitetura**:
  - Módulo novo `agents_routes.rs` (~780 linhas) + merge no
    `build_router`.
  - `append_agent_log` local inlinea `appendAgentLog` do `.cjs`
    (JSONL + `domain_graph::on_agent_log_event`).
  - Deps adicionadas em `Cargo.toml`: `domain-agents`, `domain-graph`.
  - `GraphSystemClock.now_iso()` via trait `domain_graph::Clock`.
- **Decisões**:
  - `create` retorna 200 (não 201) para paridade direta com `.cjs:1833`.
  - `mcps` array sempre presente no config (mesmo vazio), paridade com
    `.cjs:1829` que força `config.mcps = []`.
  - `doc_save`: rota `/api/docs/:doc` (sem regex), handler valida
    `^[A-Z_]+\.md$` e allowlist — paridade com regex na rota `.cjs:3020`.
  - `autonomy` embrulha `{level, call_human_when}` no payload antes de
    `update_autonomy` (paridade com `.cjs:1651`).
  - `propagate_seed=true` sem origin → 400 com `updated` no body.
  - `work`: `agent: null` quando filtro ausente (paridade `agent || null`).
- **Findings novos**: nenhum — lógica do `.cjs` mapeou limpo.
- **Paridade v2**: confirmada byte-a-byte em 32 cenários.

### ✅ `api-http` F4-05d-2 (human chat read-only) — 2026-04-19
- **2 rotas** portadas:
  - `GET /api/humans/:id/chats`            — summary de agent_chats +
    human_chats por id
  - `GET /api/human-chat/history?me=&with=` — histórico humano↔humano
- Parity tests integrados ao `parity_humans.rs` (chats_summary_*,
  human_chat_history_*) — 4 casos cobertos.
- Leituras puras via `domain_humans::{summarize_human_chats,
  read_human_chat}` — sem side effects.

### ✅ `api-http` F4-05d-1 (humans CRUD + heartbeat) — 2026-04-19
- **Crate nova** `domain-humans` criada do zero — lógica estava inline
  em `plugin/ui/server.cjs`. Contém: Clock trait + SystemClock/
  FixedClock, `slug_human` (NFD + diacritics + kebab + cap40 UTF-16),
  5 funções sanitize (location/gender/skin/position/ui_prefs),
  load/save tolerantes, CRUD + heartbeat + append-only log.
- **Tests verdes**: 16 unit tests em `domain-humans` + 19 parity
  tests `parity_humans.rs` (cross-language contra node `server.cjs`).
- **6 rotas** portadas:
  - `GET    /api/humans`                — lista + default_id
  - `POST   /api/humans`                — cria (201)
  - `GET    /api/humans/:id`            — summary
  - `PUT    /api/humans/:id`            — update parcial
  - `DELETE /api/humans/:id`            — remove + reset default_id
  - `POST   /api/humans/:id/heartbeat`  — touch last_seen_at
- **Arquitetura**:
  - Módulo novo `humans_routes.rs` + merge no `build_router`.
  - `domain-humans::Clock` é trait domain-specific (diferente de
    `domain_bus::Clock`). Handlers instanciam `HumansSystemClock`
    local por chamada (mesmo padrão de pipelines/architect).
  - Response wrapper: `create_human/update_human/touch_human`
    retornam Value bruto; handler aplica `human_summary(&h, now_ms)`
    antes de responder.
  - Update usa `body_obj.get(key).is_some()` pra decidir noop
    (paridade `body[f] === undefined` do `.cjs`).
- **Decisões**:
  - Deixei fora: chat humano↔humano (`human_chats/`) e shadow-turns
    (Claude spawn) — movidos para F4-05d-2 e F4-05f.
  - `heartbeat` responde `is_online: true` sempre (paridade direta
    com `.cjs` que não consulta clock após touch).
  - `delete`: se `default_id == id`, zera pro primeiro key restante
    ou `null` se vazio (paridade com `Object.keys()[0]`).
- **Fora de escopo**: 2 rotas extras do server.cjs (`GET /humans/:id/
  chats` e `GET /human-chat/history`) viram F4-05d-2; `/api/human-
  handoff` junta com F4-05f (precisa de spawn Claude).

### ✅ `api-http` F4-05c (destructive + seeds + bp + pipelines + architect + skills + ides) — 2026-04-19
- **Tests verdes** no crate `api-http`: 7 unit destructive + 9 parity
  destructive + 4 parity seeds + 9 parity best-practices + 16 parity
  pipelines + 9 parity misc (architect/skills/ides). Total F4-05c:
  54 novos parity tests, todos verdes (flaky só em run concorrente
  por bind race — passa serializado).
- **22 rotas** portadas de `plugin/ui/server.cjs`:
  - `/api/data` (3) — destructive reset/clear/clear-observations
  - `/api/seeds`, `/api/seeds/apply`
  - `/api/best-practices` (6 rotas CRUD + apply + catalog)
  - `/api/pipelines` (10 rotas: list/install/create/state/start/
    advance/cancel/clear/runs/:id)
  - `/api/architect/{ensure,generate}`
  - `/api/skills`
  - `/api/ides`, `/api/ides/apply`
- **Arquitetura**:
  - Módulos novos: `destructive_routes.rs`, `seeds_routes.rs`,
    `best_practices_routes.rs`, `pipelines_routes.rs`,
    `misc_routes.rs`. Cada um expõe `*_router() -> Router<AppState>`
    merged no `build_router()`.
  - `AppState.plugin_dir` adicionado — resolve `PIPE_DIR`/`SEEDS_DIR`/
    `BP_DIR` via `POUPAR_PLUGIN_DIR` env no bin com fallback
    compile-time (`CARGO_MANIFEST_DIR/../../../plugin`).
  - Parity tests setam `POUPAR_PLUGIN_DIR` pro dir real
    `plugin/` pra ambos os servers enxergarem o mesmo catálogo.
  - Tempdirs distintos por server (state fs-local pra cada run).
- **Decisões**:
  - `LocalPracticeError::InvalidId` mapeado no HTTP layer — `.cjs`
    usa msg curta em update/delete e longa em create; Rust domínio
    tem variante única, handler remapeia via `map_local_err`.
  - `domain-pipelines` usa `domain_graph::Clock` ≠ `domain_bus::Clock`
    do `AppState.clock`. Instanciamos `SystemClock` local por chamada.
  - `ides_list` retorna `{ supported: [...] }` com lista estática
    `IDE_SUPPORTED` do adapter.
  - `architect.generateSeed` `ref` (palavra reservada em Rust) → 
    `GenerateSeedResult.reference`; re-mapeado pra `ref` no JSON.
- **Findings novos**: F4-05c-01 (InvalidId error msg remap).
- **Paridade v2**: confirmada byte-a-byte em 47 cenários.

### ✅ `api-http` F4-05b (vault endpoints) — 2026-04-19
- **37 tests verdes** no crate `api-http`: 11 unit (6 existentes +
  5 unlock_guard) + 12 parity_http + 14 parity_vault, zero falhas.
- **20 rotas** portadas de `plugin/ui/server.cjs:3122-3396`:
  - `/api/vault/{status,paths,init,unlock,lock,change-password,keys}`
  - `/api/vault/secret/:name` (GET/PUT/DELETE)
  - `/api/vault/envfiles`
  - `/api/vault/envfile/:name` (GET/PUT/DELETE)
  - `/api/vault/{export,import,destroy}`
- **Arquitetura**:
  - `src/vault_routes.rs` (560 linhas) — `vault_router()` devolve
    `Router<AppState>` com `.merge()` no `build_router()` principal.
  - `src/unlock_guard.rs` (230 linhas) — state mutable + lockout
    persistido em `.vault-unlock-state.json` via atomic tmp+rename.
    Replica `server.cjs:471-541` (janela 60s, max 5 falhas, lockout
    60s). 5 unit tests cobrem fresh/lockout/expiry/reset/load.
  - `AppState` extendido com `vault: Arc<dyn VaultStore>` + 
    `unlock_guard: Arc<UnlockGuard>`. `FileVaultStore::new()` no bin.
- **Guards portados** (`guard()` chamado no início de cada handler):
  - Host header → `VAULT_ALLOWED_HOSTS` (localhost+127.0.0.1+lan/custom)
  - Origin → `VAULT_ALLOWED_ORIGINS` (same set com `http://`)
  - Sec-Fetch-Site → bloqueia qualquer coisa fora de
    `{same-origin, none, ""}`
  - Bloqueio → 403 com corpo `{"error": "..."}` idêntico ao `.cjs`.
- **Status codes mapeados** (`vault_error_response()`):
  - `Locked | NotInitialized` → 423
  - `WrongPassword | AlreadyInitialized | Validation | Io | Serde |
    Malformed` → 400
- **Tests de parity cobertos** (14 casos):
  - status vazio + paths shape
  - init happy + reject re-init
  - lock idempotente
  - keys/secret_get/secret_put locked → 423
  - envfiles/envfile_get locked → 423
  - destroy wrong confirm → 400
  - destroy locked → 423
  - guard: host rebinding → 403
  - guard: origin cross-site → 403
  - guard: sec-fetch-site cross-site → 403
- **Decisões**:
  - Tempdirs **distintos** por servidor nos parity tests (vault muta
    `vault.enc` e `.vault-devicekey` — compartilhar causaria race).
    `parity_http.rs` continua com tempdir único (só mexe em configs
    idempotentes).
  - `init` retorna shape enxuto `{initialized, expires_at}` (match
    `.cjs`) em vez do `VaultStatus` completo que o adapter devolve.
    Registrado como F4-05b-01.
  - `import` happy-path **não testado** em parity porque `.cjs`
    devolve `{ok, imported_at, exported_at}` e Rust só `{ok:true}` —
    divergência registrada em F4-05b-02.
  - `export` happy-path **não testado** em parity (bytes binários).
  - `unlock` happy-path **não testado** em parity (state mutável
    FS+TTL+clock, coberto em unit tests do adapter-vault-file).
- **Findings novos**: F4-05b-01 (init shape), F4-05b-02 (import
  metadata), F4-05b-03 (sec-fetch-site vazio).
- **Paridade v2**: confirmada byte-a-byte em 14 cenários.

### ✅ `api-http` F4-05a (scaffold + static + bus/network) — 2026-04-19
- **18 tests verdes** no crate `api-http`: 6 unit + 12 parity, zero falhas.
- Scaffold completo da crate `api-http` com Axum 0.7 + Tower + Tokio.
  Adicionadas deps no workspace (`axum`, `tower`, `tower-http`, `bytes`,
  `mime_guess`) e `if-addrs` no crate.
- 6 endpoints portados de `plugin/ui/server.cjs`:
  - `GET /` — serve `index.html` via `include_str!` (embutido no bin)
  - `GET /api/network-config` — bind/customHost/activeHost/port/lanIps
  - `PUT /api/network-config` — valida `bind` e `customHost` com
    sanitização regex `[^0-9a-zA-Z.\-:]`
  - `GET /api/bus/state?recent=N` — usa `domain_bus::get_state`
  - `GET /api/bus/trace?trace_id=X` — usa `domain_bus::get_message`
  - `GET|PUT /api/bus/config` — wrappers em `get_config`/`set_config`
  - `POST /api/bus/compact` — wrapper em `compact_old_files`
- **Padrão aplicado**: lib pura (`src/lib.rs` com `AppState`,
  `build_router`, handlers) + thin bin (`src/bin/http.rs`, só resolve
  paths + bind + serve). `AppState` injeta `Arc<dyn Clock>` pra
  testabilidade futura; `SystemClock` default em prod.
- **Parity strategy**: fixture em tempdir compartilhado, 2 instâncias
  (node `server.cjs` + `poupar-http`) em portas livres distintas, `curl`
  subprocess GET/PUT em cada uma, compara `{status, body}` com
  `strip_volatile` removendo `port`/`activeHost`/`lanIps`.
- **Tests de parity cobertos**:
  - network-config GET + PUT (lan / invalid bind / custom sem host)
  - bus/config GET defaults + PUT retention válido + fora de range
  - bus/state vazio (queue stub) + bus/trace missing/not-found
  - bus/compact vazio + serve index HTML
- **Decisões**:
  - Rustc 1.85 pin → `icu_*` precisava 1.86, troquei `reqwest` por
    `curl` subprocess (mesmo padrão do port de `api-cli`).
  - `if-addrs` substitui FFI manual de `getifaddrs` — cross-platform,
    zero deps transitivas.
  - `serve_static` permite `dist_dir` opcional (ui-next/dist quando
    existe); fallback pra `index.html` bundled.
  - Queue ainda não wired (virá em F4-05f) — injetamos stub
    `{"running":0,"pending":0,"agents":{},"rateMax":0}` no `/api/bus/state`
    pra manter shape compatível.
  - `Clock` já tem Send+Sync porque `SystemClock: Copy`; explicitei na
    trait object bound do `AppState`.
- **Fora de F4-05a** (vai nas próximas sub-fases): `/api/data` (toca
  todos os domínios), vault endpoints, destructive/seeds/pipelines,
  humans, agents, chat, SSE, scheduleNudge, runCompaction cron.
- **Findings novos**: nenhum.

### ✅ `api-mcp` (F4-04) — 2026-04-19
- **44 tests verdes** no crate `api-mcp`: 29 unit + 15 parity, zero
  falhas, zero regressão.
- 4 tools portadas de `plugin/mcp/server.cjs` (285 linhas):
  - `mem_context` — estado atual (sessions.jsonl tail + observations
    recentes + slice do grafo)
  - `mem_read_project` — dump de `PROJECT.md`
  - `mem_search` — busca em memory files (case-insensitive, filtro
    por `type`, sort por `ts` desc)
  - `mem_add_note` — append em `observations.jsonl`
- **Padrão aplicado**: lib pura (`src/lib.rs`) + thin bin
  (`src/bin/mcp.rs`). `ToolContext` trait injeta `mem_dir` + clock
  (`now_ms` + `now_iso`); `FsToolContext` impl faz I/O real. Dispatch
  JSON-RPC (`handle_rpc`) é testável via `StubCtx` sem tocar FS.
- **Parity strategy**: dois tempdirs com fixture idêntica, mesmo
  script JSON-RPC newline-delimited empilhado em `node server.cjs` e
  `poupar-mcp`, stdout parseado linha-a-linha e comparado como
  `Vec<Value>` (ordem importa — `preserve_order` garante match
  byte-a-byte de `jsonrpc`/`id`/`result`).
- **Tests de parity cobertos** (15 casos):
  - initialize handshake + tools/list (4 tools, schemas idênticos)
  - notifications/initialized silent (no response)
  - mem_read_project com arquivo ausente e presente
  - mem_context com sessions/obs/graph populados
  - mem_search: case-insensitive, filtro `type`, no-results
  - mem_add_note: persiste em observations.jsonl; rejeita body vazio
  - tool name desconhecido retorna erro
  - full session roundtrip (4 requests sequenciais)
  - método desconhecido sem id: silent
  - linha JSON malformada: descartada silenciosamente (paridade com
    `try { JSON.parse } catch { return }` do `.cjs`)
- **Decisões**:
  - `args.limit || 30` do JS replica via
    `.filter(|n| *n > 0.0).map(|n| n as usize).unwrap_or(30)` para
    manter semântica truthy (limit=0 → 30).
  - `chars().take(n)` para o slice de preview (aproximação UTF-16 via
    char boundaries — já padrão do crate).
  - `writeln!` + `flush` por mensagem no bin (paridade com
    `process.stdout.write(line + '\n')` do `.cjs`).
  - `mem_dir` recomputado por mensagem (paridade com
    `findProjectRoot(process.cwd())` chamado a cada tool call no
    `.cjs`).
- **Findings**: nenhum novo — lógica do `.cjs` mapeou limpo.

### ✅ `api-hooks` (F4-03) — 2026-04-19
- **158 tests verdes** no crate `api-hooks`: 125 unit + 33 parity
  (9 observe + 8 session-end + 8 session-init + 8 startup), zero
  falhas, zero regressão.
- Todos os 4 binários de hook portados:
  - `poupar-hook-startup` (126 linhas `.cjs`) — banner + MCP check
  - `poupar-hook-observe` (182 linhas) — PostToolUse
  - `poupar-hook-session-end` (214 linhas) — SessionEnd
  - `poupar-hook-session-init` (411 linhas) — UserPromptSubmit
- **Padrão aplicado**: lib pura (`src/*.rs`) + thin bin (`src/bin/*.rs`).
  Toda decisão testável mora na lib; bin só parseia stdin, chama lib,
  imprime stdout, toca FS/git quando necessário.
- **Parity strategy**: dois tempdirs com estado idêntico, mesmo stdin
  JSON para `.cjs` (via `node` subprocess) e bin Rust, compara
  stdout + arquivos JSONL com campos de timestamp stripados.
- **Decisões de session-init**:
  - `estimate_tokens` via `encode_utf16().count()` (UTF-16 match com
    `String.length` do JS, já resolvido no `core-paths::tokens`).
  - `render_final_context` usa `\n\n---\n` como separador entre
    blocos e rodapé `<!-- open-poupar: ~N/Mt -->` com lista de
    skipped quando budget estoura.
  - Git context: branch + modified (top 10) + 5 commits recentes
    via `Command::new("git")` — falha silenciosa se não for repo.
  - ADRs: walk em `docs/adr`, `docs/adrs`, `docs/decisions`, `adr`
    (primeira pasta existente ganha), pega últimos 8 `.md` sorted.
  - Idempotência: flag `tmp/.init-<sid>` evita reinjetar na mesma
    sessão (todos os turnos subsequentes saem silenciosos).
  - First-time (sem PROJECT.md + sem sessions.jsonl) dispara
    `init_project_config` + `onboarding_message` via
    `additionalContext`.
  - Clock trait conflict entre `domain_graph::SystemClock` e
    `domain_indexer::SystemClock` resolvido via `as IndexerClock`
    no import do session-end bin.
- **Correção de teste**: `md_block_returns_none_when_only_header`
  estava semanticamente incorreto — `.cjs` faz `trim().replace(^#\s+.*\n).trim()`
  e `"# nada\n".trim()` vira `"# nada"` (sem `\n` final), regex não
  matcha e content fica `"# nada"` (não-vazio). Renomeado para
  `md_block_returns_none_when_empty` testando whitespace puro.
- **Findings**: nenhum novo — lógica do `.cjs` mapeou limpo.
- **Fora de escopo**: autostart do `ui_server` dentro do Rust
  (F4-05 ainda não existe; `.cjs` continua responsável até lá).

### ✅ `core-queue` — 2026-04-19
- 13 unit tests + 4 parity tests verdes (Rust↔Node via `Command::new("node")`
  rodando script inline que `require`a `plugin/scripts/queue.cjs` e
  emite JSON com `{ calls, throttles }` no stdout)
- clippy limpo (com `#[allow(clippy::manual_async_fn)]` justificado),
  `cargo fmt` aplicado
- **Escopo**: motor genérico de fila de turns por agente. Port direto
  de `plugin/scripts/queue.cjs` (~149 linhas). Mantém state de
  `running` + `pending` + janela deslizante de rate-limit; drena em
  lote por agente.
- **Decisões**:
  - `setImmediate(drain)` → `tokio::spawn(drain_one)`. Contrato igual
    ("enfileira e processa depois"); exige `Arc<Self>` clone.
  - `Date.now()` → trait `Clock` injetável (`SystemClock` default usa
    `Instant::now`; `ManualClock` com `AtomicU64` pra rate-limit
    determinístico em tests).
  - Runner = `Arc<dyn Fn(String, String, String, Vec<String>) -> BoxFut
    + Send + Sync + 'static>`. Hooks = `Arc<dyn Fn(Event) + Send + Sync>`.
    Sem try/catch — runners/hooks devem ser total (panic propaga;
    divergência justificada e documentada).
  - `drain_one` com assinatura explícita `impl Future + Send` (em vez de
    `async fn`) porque é recursiva — Rust não consegue inferir Send do
    próprio corpo enquanto valida o corpo. Helper `try_acquire_slot` /
    `take_batch` / `release_slot` isolam `MutexGuard` do frame async
    (std::sync::Mutex é !Send).
  - `Arc<dyn Clock + Send + Sync + 'static>` — supertrait do Clock
    não propaga automaticamente pro trait object; precisa explícito.
  - State em `Mutex<State>` (std::sync) com lock curto. Nunca holdado
    através de await.
- **Parity tests cobertos**:
  - Single enqueue → mesma chamada do runner
  - Burst de 3 enqueues mesmo agente → batch único com `múltiplos
    agentes` e `[from]: msg` separados por `\n\n`
  - Throttle: rateMax=2, 4 enqueues → 2 runner calls + 2 throttle events
  - Acquire + enqueue + release: drain só roda após release
- **Fora de escopo**: `_running`/`_pending` expostos no `.cjs` não têm
  equivalente direto — introspecção via `stats()` / `is_running()` /
  `pending_size()`.
- **Achados novos**: nenhum — semântica mapeou limpo.

### ✅ `api-cli` — 2026-04-19
- 14 unit tests + 5 parity tests verdes (Rust↔Node via `Command::new("node")`
  rodando `scripts/{install,uninstall}.cjs` com `CLAUDE_CONFIG_DIR` env
  apontando pra tempdir isolado)
- clippy limpo, `cargo fmt` aplicado
- **Escopo**: primeiro crate inbound da Fase 4. Binário único `poupar`
  com 3 subcomandos (`install`, `uninstall`, `status`) substituindo os
  3 `scripts/*.cjs` (~160 linhas totais).
- **Decisões**:
  - Lib pura (`apply_install` / `apply_uninstall` / `describe_status`
    / `StatusReport::render`) separada do binário (thin main `poupar.rs`).
    Toda decisão testável fica na lib — binário só resolve paths,
    chama lib, escreve stdout/stderr e escolhe exit code.
  - `node_bin` substitui `process.execPath` do `.cjs` — Rust não tem
    equivalente implícito. Resolução: `--node-bin` flag → `$NODE_BIN`
    env → fallback `"node"` (PATH).
  - `settings_path()` replica `CLAUDE_CONFIG_DIR || $HOME/.claude` do
    `.cjs`. `HomeMissing` erro só se ambos faltarem (raro).
  - `serialize_settings` força indent 2 + trailing `\n` pra bater
    byte-a-byte com `JSON.stringify(x, null, 2) + '\n'` do Node.
  - `serde_json` com feature `preserve_order` (já no workspace) garante
    que inserção de `extraKnownMarketplaces` → `enabledPlugins` →
    `mcpServers` nesta ordem sobrevive ao round-trip.
  - `apply_uninstall` retorna `bool` — binário usa flag pra decidir
    entre "removido" e "nada a fazer" no output.
  - Status exit code: 0 em missing/incomplete (paridade `log + exit(0)`
    do `.cjs`); 1 apenas em parse-invalid.
- **Parity tests cobertos**:
  - Install em `settings.json` ausente: bytes idênticos
  - Install preserva `theme`, outros `enabledPlugins`, outros `mcpServers`
  - Install + uninstall round-trip volta ao fixture original (bytes iguais)
  - Uninstall em `settings.json` ausente: ambos no-op (arquivo não é criado)
  - Binário `poupar` compilado (`target/debug/poupar install …`) bate com
    Node (e2e, pulado se bin não existe)
- **Fora de escopo**: `.cjs` de upgrade/migration (não existem hoje),
  `poupar rollback`/`poupar upgrade` (features v2-only futuras).
- **Achados novos**: nenhum — semântica `.cjs` mapeou limpo.

### ✅ `adapter-fs` — 2026-04-19
- 20 unit tests + 12 parity tests verdes (Rust↔Node via `node -e`
  contra `plugin/scripts/lib.cjs` e `graph.cjs`)
- clippy limpo, `cargo fmt` aplicado
- **Escopo**: segundo adapter outbound da Fase 3. Wiring:
  - `ObservationLogFs` — JSONL append-only (`appendJsonl` /
    `readJsonl` / `readLastJsonlLine`)
  - `GraphStoreFs` — `graph.json` load/save (indent 2, ordem de
    chaves preservada via `preserve_order`)
  - `SessionStoreFs` — flag file `<tmp>/.init-<session_id>`
- **Decisões**:
  - Stateless: todos os 3 adapters são `Default + Clone + Copy`. Estado
    vive no path/tmp_dir passado em cada chamada.
  - `read_last` replica o scan backward em chunks de 2048 bytes — mesmo
    CHUNK do `.cjs`.
  - `load`/`save` do grafo são byte-minimal: sem validação de versão,
    sem mutação de `updated_at` (responsabilidade do domínio/app
    layer — ver `F20-02` e `F20-03`).
  - `mark_initialized` usa `File::create` (truncate) para ser idempotente
    — paridade com `fs.writeFileSync(flag, '')`.
  - `clear` engole `NotFound` (paridade com `try { unlink } catch {}`).
- **Parity tests cobertos**:
  - Bytes idênticos em `append` (Rust vs Node)
  - Node appends → Rust reads (`read_all`, shape idêntica)
  - Rust appends → Node reads
  - `read_last` em arquivo de 200 linhas > chunk bate com `.cjs`
  - `read_last` com tail malformado → ambos retornam null (F20-01)
  - `read_all` com mix de blank/malformed → mesma saída dos 2 lados
  - `save` graph.json com objeto complexo → bytes idênticos
  - Node saves → Rust loads; Rust saves → Node loads
  - Rust marca/limpa flag → Node enxerga corretamente (e vice-versa)
- **Fora de escopo** (caller responsibility): update de `updated_at`,
  rebuild de grafo em version mismatch, `ensureMemDir`/`ensureTmpDir`
  (helpers de filesystem genéricos — vão em app layer ou outro crate).
- **Achados novos**: `F20-01` (🟡 `readLastJsonlLine` não retrocede
  quando tail é malformado), `F20-02` (🟢 auto-migração de versão
  fica fora do adapter), `F20-03` (🟢 mutação de `updated_at` fica
  fora do adapter).

### ✅ `adapter-vault-file` — 2026-04-19
- 32 unit tests + 8 parity tests verdes (Rust↔Node via `node -e`
  contra `plugin/scripts/vault.cjs`)
- clippy limpo, `cargo fmt` aplicado
- **Escopo**: primeiro adapter outbound da Fase 3. Wiring
  `domain-vault` (pura) → disco. Replica semântica completa de
  `plugin/scripts/vault.cjs`: init/unlock/lock/change_password,
  secrets CRUD, envfiles CRUD, `get_secrets_for_agent` (sem UI),
  export/import PMVEXP1, destroy.
- **Decisões**:
  - Atomic write via `tmp-<hex>` + `rename`, com `O_NOFOLLOW | O_EXCL`
    em Unix (proteção contra symlink attack).
  - 0600 em `vault.enc` e `.vault-devicekey`.
  - `Mutex<Option<Vec<u8>>>` cacheia device-key;
    `Mutex<Option<UiSession>>` guarda TTL em memória.
  - Auth upgrade PBKDF2→scrypt pós-unlock silencioso (espelha
    `try {} catch {}` do .cjs — finding `F19-02`).
  - Preserva ordem de escrita do `.cjs` na migração `PMV1→PMV2` mesmo
    com `F19-01` aberto: fix vira issue de upstream, não deste crate,
    para manter paridade byte-level durante cutover.
  - `rand::rngs::OsRng` em rand 0.9 só implementa `TryRngCore` —
    trocado por `rand::rng()` (`ThreadRng`, seed do OS + infallible
    `RngCore + CryptoRng`).
- **Parity tests cobertos**:
  - Vault criado em Node é lido em Rust (secrets + envfile)
  - Senha errada de vault-node é rejeitada pelo Rust
  - Rust↔Node sort determinístico de `listKeys`
  - Vault criado em Rust é lido em Node (secrets + envfile)
  - Shape de `status()` (chaves e tipos) idêntico Rust↔Node
  - Bundle exportado em Node é importado em Rust
  - Bundle exportado em Rust é importado em Node
  - `.gitignore` não é re-escrito no lado Rust quando Node já criou
- **Fora de escopo** (adapter futuro/app layer): CLI `poupar rollback`
  para PMV1 backup, migração on-disk de `.legacy-backup/<ts>/`,
  cache LRU se múltiplos vaults forem abertos em paralelo.
- **Achados novos**: nenhum — todos os desvios já rastreados em
  `migration_findings.md` na fase do `domain-vault`.

### ✅ `domain-vault` — 2026-04-19
- 21 unit tests + 13 parity tests verdes
- clippy limpo, `cargo fmt` aplicado
- Escopo: cripto pura (AES-256-GCM, scrypt, PBKDF2-SHA512),
  layout binário (PMV1/PMV2/PMVEXP1), auth block, validação
- Fora de escopo: I/O (write atômico, cache de device-key, sessão UI,
  gitignore persistente) — adapter futuro
- Funções expostas como determinísticas (aceitam salt/nonce) para
  habilitar parity byte-a-byte sem mock de RNG
- Parity tests: build_header, encrypt/decrypt bidirecional,
  scrypt/PBKDF2 output cruzado, verify_password com auth block gerado
  em Node, export bundle round-trip Rust↔Node, tamper detection
- Achados registrados: `F19-01` (🔴 `importBundle` ordem das escritas
  pode brickar cofre), `F19-02` (🟡 `upgradeAuthIfNeeded` silently
  swallows error), `F19-03` (🟡 `normalizeStore` aceita array em
  .cjs — port Rust mais estrito), `F19-04` (🟢 API pura determinística),
  `F19-05` (🟢 gitignore helper puro)

### ✅ `domain-session` — 2026-04-19
- 43 unit tests + 30 parity tests verdes
- clippy limpo, `cargo fmt` aplicado
- Port domain-only extraiu funções puras dos 2 hook scripts
  (session-init/end não têm `module.exports`). I/O, stdin, chamadas
  cross-module ficam fora — adapter futuro.
- Estratégia parity: extração por range de linhas do `.cjs`,
  execução via `node -e`, comparação byte-exata.
- Achados registrados: `F18-01` (🟡 `c.exit ?? 1` trata missing como
  falha — port inicial divergiu, corrigido), `F18-02` (🟡 "undefined"
  literal em obs mal-formada), `F18-03` (🟡 precedência bugfix >
  refactor), `F18-04` (🟢 UTF-16 para estimate_tokens), `F18-05` (🟢
  summarize_session mais resiliente que .cjs)

### ✅ `domain-seed` — 2026-04-19
- 21 unit tests + 17 parity tests verdes
- clippy limpo, `cargo fmt` aplicado
- Achados registrados: `F17-01` (dedup de best_practices
  pack→agents), `F17-02` (vírgulas duplas em `only` silenciosas),
  `F17-03` (🟢 agents/ criado mesmo vazio), `F17-04` (🟢 literal
  "manager" fallback)
- Parity tests evitam best_practices/skills/ide — deps cross-module
  exigiriam mock do plugin tree; domínio puro basta
- Wizard/CLI (prompt, runWizard, parseArgs) ficam fora — adapter

### ✅ `domain-skills-install` — 2026-04-19
- 20 unit tests + 14 parity tests verdes
- clippy limpo, `cargo fmt` aplicado
- Achados registrados: `F16-01` (`.mcp.json` mal-formado sobrescrito
  sem backup), `F16-02` (frontmatter ignora keys hifenadas),
  `F16-03` (🟢 readdirSync não-ordenado — v2 ordena por nome)
- Ordem dos ports: movido pra antes de `domain-seed`, que depende dele

### ✅ `domain-pipelines` — 2026-04-19
- 42 unit tests + 21 parity tests verdes
- clippy limpo, `cargo fmt` aplicado
- Achados registrados: `F15-01` (decision typo silencioso),
  `F15-02` (run_id trailing hyphen), `F15-03` (`type: agent` lossy
  em round-trip)
- Diferença de API: `PIPE_DIR` → parâmetro `templates_dir: &Path`;
  runtime (`start`/`advance`/`cancel`) aceita `&dyn Clock`

### ✅ `domain-nudge` — 2026-04-19
- 34 unit tests + 14 parity tests verdes
- clippy limpo
- Fora de escopo (adapter/API): `runNudgeClaude`, `scheduleNudge`, `appendObservation`, `agent_logs` append
- Achados registrados: `F14-01` (template string no dedup key), `F14-02` (session_files=[] intencional)

### ✅ `domain-sigilo` — 2026-04-19
- 19 unit tests + 20 parity tests verdes
- clippy limpo
- Achado registrado: `F13-01` (askedByHuman não-usado)

### ✅ `domain-integrity` — 2026-04-19
- 9 unit tests + 8 parity tests verdes
- clippy limpo
- Sem findings registrados

---

## Histórico (concluídos — commits pendentes)

Todos os crates marcados ✅ ainda estão unstaged/staged em `git status` — commit unificado só ao final da Fase 2 ou em blocos.
