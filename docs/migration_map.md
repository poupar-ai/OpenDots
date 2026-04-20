# Migration Map — Node (.cjs) ↔ Rust

> Mapeamento vivo de cada arquivo `.cjs` do legado para os crates Rust
> de `v2/crates/`. Atualize sempre que um port mudar de estado.
>
> Fontes cruzadas:
> - [`migration_progress.md`](./migration_progress.md) — item em curso + histórico
> - [`migration_findings.md`](./migration_findings.md) — bugs e divergências encontrados
> - [`migrato_to_rust.md`](./migrato_to_rust.md) — plano macro original

## Legenda

- ✅ **Migrado** — crate Rust publicado, unit + parity tests verdes, paridade validada
- 🟡 **Parcial** — só domínio puro migrou; wiring de I/O / hook entrypoint pendente
- ⏳ **Pendente** — ainda 100% `.cjs` em runtime
- ❌ **N/A** — não será portado (decisão explícita)

---

## Visão geral

| Categoria | Arquivos | Linhas `.cjs` | % de 11.260 |
|---|---|---|---|
| ✅ Migrados (22) | vault, agents, graph, indexer, pipelines, bus, router, sigilo, integrity, destructive, best-practices, firefighter, nudge, architect, ide-adapters, seed, skills-install, lib, queue, install, uninstall, status | 5.725 | 50,8% |
| 🟡 Parciais (2) | session-init, session-end (domain puro ✅ — hook wiring pendente) | 625 | 5,6% |
| ⏳ Pendentes (4) | observe, startup, mcp/server, ui/server | 4.258 | 37,8% |
| ❌ N/A | — | — | — |

**Status atual: ~56% portado com paridade cross-language**; o grosso
do pendente (~87% das linhas pendentes) está em `plugin/ui/server.cjs`
(3.665 linhas) que será `api-http` (Axum + SSE + UI embutida).

---

## Mapa direto — `.cjs` → Rust

### `scripts/` (raiz — CLI de instalação)

| Arquivo `.cjs` | Linhas | Rust | Status |
|---|---|---|---|
| `scripts/install.cjs` | 62 | `crates/api-cli` (`pmem install`) | ✅ |
| `scripts/uninstall.cjs` | 59 | `crates/api-cli` (`pmem uninstall`) | ✅ |
| `scripts/status.cjs` | 39 | `crates/api-cli` (`pmem status`) | ✅ |

### `plugin/scripts/` — helpers e motores de domínio

| Arquivo `.cjs` | Linhas | Rust | Status |
|---|---|---|---|
| `lib.cjs` | 222 | `crates/core-paths` + `crates/adapter-fs` | ✅ |
| `vault.cjs` | 644 | `crates/domain-vault` + `crates/adapter-vault-file` | ✅ |
| `agents.cjs` | 860 | `crates/domain-agents` | ✅ |
| `graph.cjs` | 583 | `crates/domain-graph` + `crates/adapter-fs` (I/O) | ✅ |
| `indexer.cjs` | 507 | `crates/domain-indexer` | ✅ |
| `pipelines.cjs` | 453 | `crates/domain-pipelines` | ✅ |
| `bus.cjs` | 391 | `crates/domain-bus` | ✅ |
| `router.cjs` | 137 | `crates/domain-bus` (conjunto c/ bus) | ✅ |
| `sigilo.cjs` | 174 | `crates/domain-sigilo` | ✅ |
| `integrity.cjs` | 136 | `crates/domain-integrity` | ✅ |
| `destructive.cjs` | 255 | `crates/domain-destructive` | ✅ |
| `best-practices.cjs` | 253 | `crates/domain-best-practices` | ✅ |
| `firefighter.cjs` | 305 | `crates/domain-firefighter` | ✅ |
| `nudge.cjs` | 287 | `crates/domain-nudge` | ✅ |
| `architect.cjs` | 171 | `crates/domain-architect` | ✅ |
| `ide-adapters.cjs` | 185 | `crates/domain-ide-adapters` | ✅ |
| `seed.cjs` | 380 | `crates/domain-seed` | ✅ |
| `skills-install.cjs` | 125 | `crates/domain-skills-install` | ✅ |
| `queue.cjs` | 149 | `crates/core-queue` | ✅ |
| `session-init.cjs` | 411 | `crates/domain-session` (puro) + **`api-hooks` (pendente)** | 🟡 |
| `session-end.cjs` | 214 | `crates/domain-session` (puro) + **`api-hooks` (pendente)** | 🟡 |
| `observe.cjs` | 182 | **`api-hooks` (pendente)** | ⏳ |
| `startup.cjs` | 126 | **`api-hooks` (pendente)** | ⏳ |

### `plugin/mcp/` — servidor MCP

| Arquivo `.cjs` | Linhas | Rust | Status |
|---|---|---|---|
| `mcp/server.cjs` | 285 | **`api-mcp` (pendente)** — stdio, tools `mem.*` | ⏳ |

### `plugin/ui/` — servidor HTTP + UI

| Arquivo `.cjs` | Linhas | Rust | Status |
|---|---|---|---|
| `ui/server.cjs` | 3.665 | **`api-http` (pendente)** — Axum + Tower + SSE + UI via `include_str!` | ⏳ |

---

## Mapa inverso — crate Rust → `.cjs` de origem

### Camada `domain-*` (17 crates — lógica pura, zero I/O)

| Crate | Cobre | Nota |
|---|---|---|
| `domain-vault` | `vault.cjs` | Cripto pura; adapter separado fez o wiring de disco |
| `domain-agents` | `agents.cjs` | Maior port de domínio — 860 linhas |
| `domain-graph` | `graph.cjs` | Save/load fica em `adapter-fs` |
| `domain-indexer` | `indexer.cjs` | |
| `domain-pipelines` | `pipelines.cjs` | Clock injetado |
| `domain-bus` | `bus.cjs` + `router.cjs` | Unificados num crate |
| `domain-sigilo` | `sigilo.cjs` | |
| `domain-integrity` | `integrity.cjs` | |
| `domain-destructive` | `destructive.cjs` | |
| `domain-best-practices` | `best-practices.cjs` | |
| `domain-firefighter` | `firefighter.cjs` | |
| `domain-nudge` | `nudge.cjs` | |
| `domain-architect` | `architect.cjs` | |
| `domain-ide-adapters` | `ide-adapters.cjs` | |
| `domain-seed` | `seed.cjs` | |
| `domain-skills-install` | `skills-install.cjs` | |
| `domain-session` | `session-init.cjs` + `session-end.cjs` (funções puras) | **🟡 Hook entrypoint e I/O ainda em `.cjs`** |

### Camada `core-*` (4 crates — primitivas e ports)

| Crate | Cobre | Nota |
|---|---|---|
| `core-paths` | `lib.cjs` (paths, JSONL, stats, config, CLAUDE.md, stdin) | Port direto e completo |
| `core-queue` | `queue.cjs` | Motor genérico de fila com runner pluggable |
| `core-ports` | — | Traits de adapters (`ObservationLog`, `GraphStore`, `SessionStore`, `VaultStore`, …) — sem equivalente `.cjs` |
| `core-branding` | — | Strings/constantes de branding — sem equivalente `.cjs` |

### Camada `adapter-*` (2 crates com equivalente `.cjs`; mais planejados)

| Crate | Cobre | Nota |
|---|---|---|
| `adapter-vault-file` | `vault.cjs` (lado I/O: atomic write, device-key cache, UI session) | Complementa `domain-vault` |
| `adapter-fs` | `lib.cjs` + `graph.cjs` + `session-init/end.cjs` (lado filesystem) | JSONL append/read/read_last, graph load/save, session flag |

### Camada `api-*` (1 crate — mais 3 pendentes)

| Crate | Cobre | Nota |
|---|---|---|
| `api-cli` | `scripts/install.cjs` + `uninstall.cjs` + `status.cjs` | Binário único `pmem` com 3 subcomandos |
| **`api-hooks`** (pendente) | `session-init.cjs` + `session-end.cjs` + `observe.cjs` + `startup.cjs` | 4 binários thin que leem stdin JSON, chamam `domain-session` + adapters |
| **`api-mcp`** (pendente) | `mcp/server.cjs` | Servidor MCP stdio, expõe tools `mem.*` |
| **`api-http`** (pendente) | `ui/server.cjs` | Axum + SSE + UI embutida |

---

## Pendente em detalhe (Fase 4)

Ordem de port foi decidida por tamanho/acoplamento (menor → maior,
isolado → acoplado) — ver tabela em [`migration_progress.md`](./migration_progress.md).

### F4-03 — `api-hooks` (~933 linhas, **próximo**)

4 binários thin que substituem os hook entrypoints do Claude Code:

| Binário | Origem | Linhas | O que faz |
|---|---|---|---|
| `pmem-hook-session-init` | `session-init.cjs` | 411 | Carrega sessão, popula agentes, dispara seed se primeira vez |
| `pmem-hook-session-end` | `session-end.cjs` | 214 | Resume sessão, computa stats, emite observação de fechamento |
| `pmem-hook-observe` | `observe.cjs` | 182 | Captura tool calls do Claude e grava observação JSONL |
| `pmem-hook-startup` | `startup.cjs` | 126 | Inicializa `.claude-project-memory/` se não existe |

Primeira etapa real de cutover — onde o `.cjs` efetivamente roda no
runtime do Claude Code. Todo o domínio (`domain-session`, `domain-bus`,
etc.) e ports (`core-paths`, `adapter-fs`) já estão prontos; falta
apenas o main thin que lê stdin e orquestra.

### F4-04 — `api-mcp` (~285 linhas)

Servidor MCP stdio que expõe tools `mem.*` (read/write/query de
memória) para o Claude Code. Depende de `domain-graph`, `domain-bus`,
`core-paths`, `adapter-fs`.

### F4-05 — `api-http` (~3.665 linhas — o grande)

Porto de `plugin/ui/server.cjs`: server HTTP + SSE + UI embutida. Em
Rust: Axum + Tower + `include_str!` para assets. Depende de `core-queue`,
`domain-bus`, `domain-agents`, e praticamente todos os demais domínios.

---

## Crates Rust sem origem `.cjs`

Crates que existem no workspace mas **não** portam código legado — são
fundação nova do v2:

- `core-ports` — traits abstratas (`ObservationLog`, `GraphStore`,
  `SessionStore`, `VaultStore`) que adapters implementam.
- `core-branding` — constantes de naming/strings do produto.
- *(futuros)* `adapter-firebase`, `adapter-telemetry`, `adapter-update`,
  `adapter-notifier` — v2-only; só entram depois do cutover e do modelo
  comercial começar.

---

## Histórico

- **2026-04-19** — documento criado. 22 `.cjs` completos, 2 parciais
  (hooks de sessão), 4 pendentes (`observe`, `startup`, `mcp/server`,
  `ui/server`). Próximo item: `api-hooks` (F4-03).
