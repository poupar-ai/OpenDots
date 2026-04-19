# Migração project-mem → Rust (hexagonal)

> Plano de migração do plugin Node.js (CJS) para binário Rust distribuído.
> Status: planejamento. Nenhuma fase iniciada.
> Atualizado em: 2026-04-19.

---

## 1. Objetivo

Portar todo o produto project-mem (~11.100 linhas .cjs) para Rust, em arquitetura hexagonal, para permitir:

- **Distribuição como binário fechado** (não open-source) — usuário instala via `curl` / installer nativo.
- **Segurança**: code signing OS-level, master key embutido, licença server-side.
- **Baixo custo**: roda local-first na máquina do cliente. O servidor só valida licença e assina updates.
- **Acoplar/desacoplar adapters**: trocar storage, auth, telemetria sem mexer no domínio.

**Nome interno do projeto continua `project-mem`.** Nome de binário, UI, URLs de update, logo e domínio vêm de um arquivo `settings.toml` (ou `branding.toml`) — permite rebrand sem rebuild do core.

### 1.1. Regra não-negociável — paridade 100%

**O binário Rust deve funcionar igual ao `.cjs` atual. Zero breaking changes observáveis pelo usuário.**

Isso vale para:

- **Hooks do Claude Code** (`SessionStart`, `UserPromptSubmit`, `PostToolUse`, `SessionEnd`) — mesmo stdout/exitCode, mesmos effects em disco.
- **REST** `/api/v1/*` — mesmos paths, métodos, headers, status codes, bodies (byte-a-byte) e ordem de chaves em JSON quando houver clientes sensíveis.
- **MCP stdio** — mesmos tools (`mem.*`), mesmos schemas, mesmo handshake.
- **Comandos CLI** (`pmem install`, `status`, `uninstall`) — mesmo output visual, mesmos exit codes, mesmos side-effects em `~/.claude/settings.json`.
- **Arquivos em disco** — mesma estrutura (`.claude-project-memory/`, `tmp/`, JSONL append-only), mesmos formatos de vault: lê `PMV1` (legado, encriptado por senha) e `PMV2` (atual, device-key + scrypt auth); grava `PMV2`. `PMVEXP1` para bundles de export/import. Nada perdido.
- **UI** — **zero mudança no front.** O `plugin/ui/index.html` (1.390 linhas, HTML/CSS/JS inline auto-contido) é embutido no binário via `include_str!` e servido por Axum byte-a-byte igual ao hoje.
- **Skills** (`/mem-setup`, `/mem-search`, `/mem-ui`) — mesmo contrato.
- **Eventos de bus, sigilo, integrity, firefighter** — mesmos gatilhos, mesmos payloads.

**Como garantimos isso:**

1. **Testes por fase** (obrigatório, não-negociável) — cada fase entrega (a) testes unitários cobrindo o código portado, (b) testes de paridade que rodam `.cjs` e Rust no mesmo input e comparam outputs byte-a-byte, (c) correção dos desvios detectados. Fase só é dada como concluída quando ambos os conjuntos estão verdes. A Fase 1 é exceção parcial: ports são apenas traits, não há lógica para comparar — paridade entra a partir da Fase 2 (`core-paths` portando `lib.cjs`).
2. **Testes de paridade cross-language** (#74) — runner dedicado que spawna `node <script>.cjs` e binário Rust contra os mesmos fixtures, diff byte-a-byte nas saídas (stdout, arquivos, JSON responses).
3. **Golden files** — snapshots de respostas REST, tool responses MCP, arquivos gerados em disco.
4. **Cutover só depois de paridade** (#76) — `.cjs` permanece na árvore até o teste de paridade estar 100% verde.
5. **Rollback** (`pmem rollback`) — desfaz migração de vault se algo der errado.

---

## 2. Mapeamento do código atual

| Camada Node.js | Arquivo | Linhas | Vira em Rust |
|---|---|---|---|
| HTTP/SSE/UI | `plugin/ui/server.cjs` | 3.665 | `crates/api-http` (Axum + Tower) |
| MCP stdio | `plugin/mcp/server.cjs` | 285 | `crates/api-mcp` |
| Vault | `plugin/scripts/vault.cjs` | 644 | `crates/domain-vault` |
| Agents | `plugin/scripts/agents.cjs` | 860 | `crates/domain-agents` |
| Graph | `plugin/scripts/graph.cjs` | 583 | `crates/domain-graph` |
| Indexer | `plugin/scripts/indexer.cjs` | 507 | `crates/domain-indexer` |
| Pipelines | `plugin/scripts/pipelines.cjs` | 453 | `crates/domain-pipelines` |
| Bus | `plugin/scripts/bus.cjs` | 391 | `crates/domain-bus` |
| Session init/end/observe/startup | `session-*.cjs`, `observe.cjs`, `startup.cjs` | ~933 | `crates/domain-session` |
| Seed | `seed.cjs` | 380 | `crates/domain-seed` |
| Firefighter | `firefighter.cjs` | 305 | `crates/domain-firefighter` |
| Nudge | `nudge.cjs` | 287 | `crates/domain-nudge` |
| Destructive patterns | `destructive.cjs` | 255 | `crates/domain-destructive` |
| Best practices | `best-practices.cjs` | 253 | `crates/domain-best-practices` |
| Lib / helpers | `lib.cjs` | 222 | `crates/core-paths` |
| IDE adapters | `ide-adapters.cjs` | 185 | `crates/adapter-ide` |
| Observe | `observe.cjs` | 182 | (absorvido em domain-session) |
| Architect | `architect.cjs` | 171 | `crates/domain-architect` |
| Queue | `queue.cjs` | 149 | `crates/core-queue` |
| Integrity | `integrity.cjs` | 136 | `crates/domain-integrity` |
| Router (bus) | `router.cjs` | 137 | (absorvido em domain-bus) |
| Skills install | `skills-install.cjs` | 125 | `crates/adapter-skills` |
| Sigilo | `sigilo.cjs` | 174 | `crates/domain-sigilo` |
| Install CLI | `scripts/install.cjs` | 62 | `crates/api-cli` (`pmem install`) |
| Uninstall CLI | `scripts/uninstall.cjs` | 53 | `crates/api-cli` (`pmem uninstall`) |
| Status CLI | `scripts/status.cjs` | 43 | `crates/api-cli` (`pmem status`) |

**Total portado**: ~10.800 linhas de domínio + ~300 linhas de infra + ~160 linhas de CLI raiz.

---

## 3. Arquitetura hexagonal

### 3.1. Camadas

```
┌─────────────────────────────────────────────────────────────┐
│  Adapters (inbound)                                         │
│  api-http (Axum) · api-mcp (stdio) · api-cli (clap)         │
└──────────────────────────┬──────────────────────────────────┘
                           │ usa ports
┌──────────────────────────▼──────────────────────────────────┐
│  Application (use-cases)                                    │
│  crates/app-* — orquestra domínio + ports                   │
└──────────────────────────┬──────────────────────────────────┘
                           │ define ports (traits)
┌──────────────────────────▼──────────────────────────────────┐
│  Domain                                                     │
│  crates/domain-* — regras puras, sem I/O                    │
└──────────────────────────┬──────────────────────────────────┘
                           │ implementa ports
┌──────────────────────────▼──────────────────────────────────┐
│  Adapters (outbound)                                        │
│  adapter-fs · adapter-firebase · adapter-stripe             │
│  adapter-telemetry · adapter-ide · adapter-skills           │
└─────────────────────────────────────────────────────────────┘
```

### 3.2. Ports (traits) principais

- `VaultStore` — ler/escrever arquivos versionados (lê `PMV1` legado + `PMV2` atual; escreve `PMV2`; import/export `PMVEXP1`)
- `GraphStore` — nós/edges persistidos
- `ObservationLog` — JSONL append-only
- `SessionStore` — estado por sessão
- `Clock` — `now()` mockável
- `LicenseValidator` — valida JWT Ed25519 + status server-side
- `UpdateChannel` — busca manifest assinado
- `TelemetrySink` — eventos opt-in
- `Notifier` — push de bus/router

### 3.3. Stack técnica

- **Runtime**: `tokio` (multi-thread, features full).
- **HTTP**: `axum` + `tower` + `tower-http`.
- **SSE**: `axum::response::sse` + `tokio::sync::broadcast`.
- **MCP**: `tokio::io` stdio + `serde_json` (JSON-RPC 2.0 sem framework — protocolo fixo).
- **Serialização**: `serde` + `serde_json` + `toml` + `jsonl-rs` (ou manual).
- **Crypto**: `ed25519-dalek` (licença), `ring` (code signing, assinatura manifest).
- **Erros**: `thiserror` (domínio) + `anyhow` (app/adapters).
- **Logs**: `tracing` + `tracing-subscriber` (JSON).
- **Config**: `figment` ou `config-rs` (TOML + env + CLI override).
- **Testes**: `insta` (snapshot), `proptest` (invariantes), `wiremock` (adapters HTTP).

---

## 4. Compatibilidade com instalações existentes

### 4.1. Formato do vault

- **Lê** `PMV1` (legado, AES-GCM com key derivada da senha via PBKDF2) **e** `PMV2` (atual, AES-GCM com `device-key` aleatória de 32B em arquivo 0600, auth via scrypt — fallback PBKDF2 p/ cofres ainda não upgraded).
- **Escreve** `PMV2`. Nenhum formato novo é introduzido pela port.
- **Migração lazy `PMV1` → `PMV2`** no primeiro `unlock(password)` (igual ao `.cjs`): decripta com senha, gera device-key, re-encripta, grava auth block scrypt.
- **Backup** do arquivo `vault.enc` antes de migrar: `.legacy-backup/<timestamp>/vault.enc`.
- **Rollback** via `pmem rollback <timestamp>`.
- **Bundles de export/import** (`PMVEXP1`): AES-GCM com key derivada via scrypt da senha do bundle — compatibilidade byte-a-byte com o formato emitido pelo `.cjs`.

### 4.2. API REST

- Prefixo `/api/v1/*` preservado com **shapes idênticos** (50+ handlers).
- Testes de paridade: rodar `.cjs` e Rust em paralelo, comparar respostas byte-a-byte.

### 4.3. MCP

- Tools `mem.*` preservadas com mesmo contrato.
- Handshake `initialize` + `tools/list` idêntico.

---

## 5. Licenciamento e updates

### 5.1. Chaves

- **1 master keypair Ed25519** — privada no Firebase (Cloud Function + KMS), pública embutida em todos os binários.
- **Build único** — mesmo binário para todos os clientes (sem rebuild por compra).
- **Per-tenant key** fica como evolução v2 (se master vazar, força update com nova key).

### 5.2. Modelo

```
1 licença = 1 desenvolvedor
└── N máquinas (limite por tier)
    └── ∞ projetos usando o binário
```

- Licença **não amarra projeto** — amarra máquina.
- `machine_id = SHA-256(hostname + MAC primária + OS + CPU model)` — estável, não identifica pessoa.

### 5.3. Tiers

| Tier | Installs | Updates | Suporte |
|---|---|---|---|
| Free | 1 | Nenhum | Comunidade |
| Basic | 1 | Security + bugfix | Comunidade |
| Pro | 3 | Features minor | Email |
| Business | 10 | Major + SLA | Priority + SLA |

### 5.4. Endpoints (Firebase Cloud Functions)

| Método | Rota | Descrição |
|---|---|---|
| POST | `/license/activate` | Registra máquina, retorna licença assinada |
| POST | `/license/check` | Status atual (plan, expires_at). Rate-limited. |
| POST | `/license/deactivate` | Libera slot manualmente |
| POST | `/update/sign` | Se plan permite, retorna manifest assinado |
| GET | `/update/manifest/:channel` | Metadata público (sem sig) |

### 5.5. Caches e fluxo

| Fluxo | Valida | Cache offline |
|---|---|---|
| Usar o software | `/license/check` 1×/dia | Sim, grace **7 dias** |
| Receber update | `/update/sign` toda vez | Não — sem servidor, sem update |

### 5.6. Resposta `/license/check` (assinada)

```json
{
  "license_id": "...",
  "plan": "pro",
  "status": "active",
  "issued_at": 1745000000,
  "expires_at": 1745604800,
  "max_installs": 3,
  "machine_id": "...",
  "signature": "ed25519:..."
}
```

---

## 6. Configuração (`settings.toml`)

Permite rebrand completo sem rebuild.

```toml
[product]
internal_name = "project-mem"   # fixo no código
display_name  = "Catclaw"        # aparece em UI, banner, CLI help
binary_name   = "catclaw"        # nome do binário compilado
logo_path     = "assets/logo.png"

[urls]
website  = "https://catclaw.ai"
docs     = "https://docs.catclaw.ai"
dashboard = "https://app.catclaw.ai"
update   = "https://updates.catclaw.ai"
api      = "https://api.catclaw.ai"

[build]
channel = "stable"    # stable | beta | nightly
target  = "x86_64-unknown-linux-gnu"
```

---

## 7. TODO — 47 tasks em 10 fases

> IDs preservam a numeração usada nas tasks do tracker (#44–#90).

### Fase 0 — Fundação

- [ ] **#44** — Setup workspace Cargo (monorepo `crates/`, root `Cargo.toml`, toolchain `rust-toolchain.toml`, CI).
- [ ] **#78** — Sistema de branding via `settings.toml` (crate `core-branding`, carregamento único, injeção em UI/CLI/URLs).

### Fase 1 — Hexagonal

- [ ] **#45** — Definir traits de portas (`VaultStore`, `GraphStore`, `ObservationLog`, `SessionStore`, `Clock`, `LicenseValidator`, `UpdateChannel`, `TelemetrySink`, `Notifier`).  ⟶ depende de #44
- [ ] **#46** — Adapters genéricos básicos (`adapter-fs`, `adapter-memory` para testes, `adapter-http-client`).  ⟶ depende de #45

### Fase 2 — Domínios (12 portados)

- [ ] **#47** — Vault (lê `PMV1`/`PMV2`, escreve `PMV2`, migração `PMV1`→`PMV2` no unlock, bundle `PMVEXP1`).  ⟶ depende de #46
- [ ] **#48** — Agents (860 linhas — orquestração, sandbox de tools).  ⟶ depende de #46
- [ ] **#49** — Graph (nós/edges, índice invertido).  ⟶ depende de #46
- [ ] **#50** — Indexer.  ⟶ depende de #46
- [ ] **#51** — Pipelines.  ⟶ depende de #46
- [ ] **#52** — Bus + Router (event bus, sweep 30s).  ⟶ depende de #46
- [ ] **#53** — Sigilo (bloqueio de vazamento de arquitetura).  ⟶ depende de #46
- [ ] **#54** — Integrity watcher.  ⟶ depende de #46
- [ ] **#55** — Destructive patterns.  ⟶ depende de #46
- [ ] **#56** — Best-practices.  ⟶ depende de #46
- [ ] **#57** — Firefighter + Nudge.  ⟶ depende de #46
- [ ] **#58** — Seed + Architect + IDE adapters + Skills install + Session lifecycle.  ⟶ depende de #46

### Fase 3 — Compatibilidade

- [ ] **#59** — Migração legacy (`PMV1` → `PMV2` com backup + rollback CLI).  ⟶ depende de #47
- [ ] **#60** — Paridade API v1 (shapes, headers, status codes, erros).  ⟶ depende de #47–#58

### Fase 4 — HTTP/SSE

- [ ] **#61** — `api-http` Axum: 50+ handlers REST.  ⟶ depende de #60
- [ ] **#62** — Chat streaming (SSE + broadcast).  ⟶ depende de #61

### Fase 5 — Licença, billing e telemetria

- [ ] **#63** — `crate license`: JWT Ed25519, verificação offline, grace 7 dias.  ⟶ depende de #44
- [ ] **#64** — Client Firebase (activate/check/deactivate). Retry + backoff.  ⟶ depende de #63
- [ ] **#65** — Tiers e enforcement (install limit, update permission).  ⟶ depende de #64
- [ ] **#66** — Update channel: `/update/sign`, assinatura do manifest, aplicação atômica.  ⟶ depende de #65
- [ ] **#77** — Machine fingerprint (`machine_id`).  ⟶ depende de #63
- [ ] **#79** — Cloud Functions (Firebase) — `activate`, `check`, `deactivate`, `update/sign`.
- [ ] **#80** — Stripe integration (checkout, webhook → ativa plano no Firebase).  ⟶ depende de #79
- [ ] **#81** — Telemetria opt-in (eventos mínimos, transparente, desligável).  ⟶ depende de #65

### Fase 6 — Build e distribuição

- [ ] **#67** — Cross-compile: linux-x86_64, linux-aarch64, macos-x86_64, macos-aarch64, windows-x86_64.
- [ ] **#68** — Code signing OS-level (Apple notarization, Authenticode, Linux sig + checksum).  ⟶ depende de #67
- [ ] **#69** — Hardening (strip, LTO, panic=abort, `#![forbid(unsafe_code)]` onde aplicável).  ⟶ depende de #67
- [ ] **#70** — Crypto de assets embutidos (logo/config sensível).  ⟶ depende de #67
- [ ] **#71** — Installers: `curl | sh`, `.pkg`, `.msi`, `homebrew`, `scoop`.  ⟶ depende de #68

### Fase 7 — Frontend e web

- [ ] **#72** — UI embarcada no binário (Axum serve `plugin/ui/index.html` via `include_str!`, sem alteração no front).  ⟶ depende de #61
- [ ] **#73** — Endpoint `/api/v1/branding` lê `settings.toml` (front consome sem mudança estrutural, apenas rebrand dinâmico).  ⟶ depende de #72
- [ ] **#82** — Dashboard (app.\*) — SPA Firebase Auth, lista máquinas, gerencia licença.  ⟶ depende de #79
- [ ] **#83** — Site institucional Next.js SSG (home, features, pricing, changelog, blog).  ⟶ depende de #78
- [ ] **#84** — Docs públicos (Astro Starlight ou Mintlify).  ⟶ depende de #78

### Fase 8 — Qualidade

- [ ] **#74** — Testes de paridade .cjs ↔ Rust (80+ golden files).  ⟶ depende de #60
- [ ] **#75** — Audit `cargo audit` + `cargo deny` + SAST.  ⟶ depende de #67
- [ ] **#85** — LGPD: dados coletados, retenção, direito de esquecimento.  ⟶ depende de #81
- [ ] **#86** — Crash reporter opt-in (Sentry ou self-hosted).  ⟶ depende de #81
- [ ] **#90** — Benchmarks (latência de handlers, throughput bus, memória).  ⟶ depende de #61

### Fase 9 — GA

- [ ] **#87** — Onboarding wizard (primeira execução sem licença → abre browser em `[urls.dashboard]/signup`).  ⟶ depende de #82
- [ ] **#88** — Status page + suporte (link/formulário em `[urls.website]/support`).  ⟶ depende de #83
- [ ] **#89** — Changelog automatizado (release-please ou equivalente).  ⟶ depende de #67
- [ ] **#76** — Cutover: desativar .cjs, redirect instalações legacy, anúncio.  ⟶ depende de #74, #87, #88, #89

---

## 8. Caminho crítico

```
#44 → #45 → #46 → (#47…#58 em paralelo) → #59/#60 → #61 → #62
                                               │
                                               └──> #72/#73
#44 → #63 → #64 → #65 → #66
              │      │
              │      └──> #81 → #85/#86
              └──> #77
#79 → #80 → #82 → #87
#78 → #83/#84
#67 → #68 → #71 → #76
```

Gargalo real está em **#47–#58** (fase 2). São 12 domínios, alguns grandes (Agents 860 linhas, Vault 644, Graph 583). Dá pra paralelizar se mais de um dev atacar. Solo: ~4–6 semanas só essa fase.

---

## 9. Estrutura de diretórios proposta

Todo o port fica isolado em `v2/` para **não tocar na versão atual em produção**. O `.cjs` segue funcionando normalmente até a paridade estar 100% (#74) e o cutover ser feito (#76).

```
project-mem/
├── plugin/                 # .cjs em produção — NÃO MEXER até cutover (#76)
├── scripts/                # .cjs install/status/uninstall — NÃO MEXER até cutover (#76)
├── plugin/ui/              # `index.html` atual — embutido no binário Rust, sem alterar front
├── docs/
│   └── migrato_to_rust.md  # este documento
├── v2/                   # NOVO — todo o port isolado
│   ├── crates/
│   │   ├── core-branding/
│   │   ├── core-paths/
│   │   ├── core-queue/
│   │   ├── domain-vault/
│   │   ├── domain-agents/
│   │   ├── domain-graph/
│   │   ├── domain-indexer/
│   │   ├── domain-pipelines/
│   │   ├── domain-bus/
│   │   ├── domain-sigilo/
│   │   ├── domain-integrity/
│   │   ├── domain-destructive/
│   │   ├── domain-best-practices/
│   │   ├── domain-firefighter/
│   │   ├── domain-nudge/
│   │   ├── domain-architect/
│   │   ├── domain-seed/
│   │   ├── domain-session/
│   │   ├── adapter-fs/
│   │   ├── adapter-firebase/
│   │   ├── adapter-stripe/
│   │   ├── adapter-telemetry/
│   │   ├── adapter-ide/
│   │   ├── adapter-skills/
│   │   ├── api-http/
│   │   ├── api-mcp/
│   │   ├── api-cli/
│   │   ├── license/
│   │   └── app/            # composition root (DI manual)
│   ├── tests/              # paridade .cjs ↔ Rust, goldens
│   ├── Cargo.toml          # workspace root
│   ├── rust-toolchain.toml
│   ├── settings.toml       # branding/URLs/build
│   ├── Makefile
│   └── README.md
├── dashboard/              # novo (Next.js SPA) — independente do port
├── site/                   # novo (Next.js SSG) — independente do port
└── docs-site/              # novo (Astro Starlight) — independente do port
```

**Regra de coexistência:**

- Durante a migração, `plugin/` e `scripts/` continuam sendo a versão oficial. Qualquer bugfix urgente do `.cjs` vai lá.
- `v2/` nunca é rodado em produção até #74 (paridade verde) e #76 (cutover).
- Hooks de `~/.claude/settings.json` apontam para `.cjs` até o cutover.
- Testes de paridade (#74) rodam os dois binários lado a lado contra o mesmo input.

---

## 10. Decisões abertas

| Tópico | Questão | Decisão pendente |
|---|---|---|
| Nome comercial | Catclaw.ai? Outro? | Adiado — build lê de `settings.toml` |
| Domínios | Checar `.ai`, `.dev`, `.so` | Antes de #78 |
| Trademark | Busca INPI/USPTO para "catclaw" | Antes de fabricar merch |
| Per-client key | Master único ou per-tenant? | Master único na v1. Per-tenant = v2 se master vazar. |
| UI renderer | Tauri (wrap nativo) ou Axum + browser? | **Axum + browser** (evita bundle de 40MB do Chromium) |
| Frete de updates | Hospedar no Firebase Storage ou CDN próprio? | Firebase Storage na v1 |

---

## 11. Próxima ação

**Iniciar Fase 0:**

1. `#44` — Criar workspace Cargo (`Cargo.toml` root, `rust-toolchain.toml`, CI mínimo).
2. `#78` — Criar `settings.toml` + crate `core-branding`.

Após isso, pedir aprovação antes de Fase 1.
