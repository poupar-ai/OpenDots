# Evolução & integrações — o que ainda não está pronto

Revisão com dois focos: **(A)** não quebrar quem já usa numa atualização e
**(B)** preparar para integrar com mensageria (SQS/SNS), storage (Google
Drive/S3), tokens de IA multi-provider e APIs de serviços externos.

> **Base que já ajuda:** ports em `core-ports/` (`VaultStore`, `ObservationLog`,
> `GraphStore`, `Notifier`, `TelemetrySink`, `Clock`, `SessionStore`,
> `UpdateChannel`), event log em `domain-bus` (`emit`/`read_events`), migração
> automática no vault (PMV1→PMV2), snapshots tar.gz em `db_routes`. A
> arquitetura aceita os adapters abaixo sem reescrever o núcleo.

---

## Regras de compatibilidade (VIGENTES — seguir sempre)

Estas já valem (alicerce implementado). Toda mudança futura respeita:

1. **API — `unversioned == v1`.** As rotas atuais (`/api/...`) são a v1 implícita
   e só mudam de forma **aditiva** (campo/rota novo ok; renomear/remover/trocar
   significado de uma rota existente = quebra). Breaking → cria `/api/v2/...` e
   **deixa as v1 congeladas** (cliente velho segue, novo migra). Nunca mudar o
   shape de uma rota já publicada "por baixo".
2. **JSON de estado — `schema_version`.** Ausência == v1 (baseline). Use
   `core_paths::schema::{read_version, stamp, CURRENT}`. `humans.json` já é
   carimbado; arquivos legados que já tinham `version` (`agents.json`,
   `config.json`, `graph.json`) seguem com o campo deles. Migração é **lazy**
   (no `load`), só quando um shape mudar — e nunca remove/renomeia campo sem ela.
3. **JSONL — aditivo.** Logs append-only (`observations.jsonl`,
   `agent_chats/*.jsonl`, `messages/*.jsonl`): um campo novo por linha é sempre
   seguro; **um reader nunca pode exigir um campo novo** (trata ausência como
   default). Campo deprecado fica (não apaga) por uma janela.
4. **JWT — claim `ver`.** Tokens carregam `ver` (`agent_keys::CLAIMS_VER`,
   default 1). Mudança de formato/algoritmo ramifica por `ver` em vez de
   invalidar tudo.

---

## A) Riscos de compatibilidade (resolver ANTES de ter usuários reais)

### A1. API sem versionamento — **convenção adotada (ver Regras vigentes)**
Decisão: começar **sem prefixo** (rotas atuais = v1 implícita, aditivas) e só
criar `/api/v2/...` quando houver breaking real / API pública externa. Prefixar
agora seria prematuro (sem cliente externo, UI acoplada). Nada a fazer no código
até lá — só seguir a regra aditiva.

### A2. Versionamento de dados — **baseline feito ✅**
Helper `core_paths::schema` criado; `humans.json` carimba `schema_version`;
arquivos que já tinham `version` (agents/config/graph) seguem com o deles.
**Migração lazy ainda não escrita** (de propósito — nasce no 1º shape que mudar,
no `load` correspondente). Regra vigente: nunca renomear/remover campo sem
migração.

### A3. Contrato dos JSONL — **médio**
JSONL append-only é naturalmente aditivo, mas falta o contrato escrito.
→ Documentar "campos são aditivos; um reader nunca exige campo novo". Marcar
campos deprecados (sem apagar) por uma janela.

### A4. Token/JWT sem versão — **`ver` feito ✅ (resta rotação)**
Claim `ver` adicionado (`agent_keys::CLAIMS_VER`, default 1) — mudar
formato/algoritmo agora ramifica por `ver`. **Falta** plano de rotação de chave
(regenerar keypair invalida JWTs — `agents_routes.rs:105` já alerta) — quando
necessário.

### A5. `authorize()` binário — **médio**
Ver `docs/permissoes-granulares.md`. Subir pra RBAC depois é aditivo (campo
`role`), então é seguro — só precisa estar previsto (já está).

---

## B) Não pronto pra integrar — ports a introduzir

A regra: **criar o port (trait em `core-ports`) primeiro**, com um adapter local
default; integrações externas viram só mais um adapter. Sem isso, integrar
depois força refactor do núcleo (= breaking).

### B1. Mensageria (SQS / SNS) — port `EventPublisher`
Já existe `domain-bus` (`emit`) — é o gancho natural. → Introduzir
`core-ports::EventPublisher { publish(topic, event) }`. O default escreve no bus
local (comportamento atual); um adapter `adapter-sqs`/`adapter-sns` publica os
mesmos eventos (padrão **outbox**: bus local é a fonte, publisher espelha).
Eventos com `schema_version` + `type` estável.

### B2. Storage remoto (Google Drive / S3) — port `BlobStore`
`db_routes` já gera snapshots tar.gz **locais**. → `core-ports::BlobStore
{ put/get/list }`; adapters `adapter-fs` (atual), `adapter-gdrive`, `adapter-s3`.
Backup/restore e (futuro) sync passam a escolher o backend por config — sem
mexer no fluxo de snapshot.

### B3. Tokens de IA multi-provider — port `ModelProvider`
Hoje o agente tem `model` mas o runner é mono-provider (`agent_runner.rs:171` e
`chat_send_routes.rs:942` marcam "plano de migração quando vier multi-provider";
ver `docs/adr-001-multi-modelo.md`). → `core-ports::ModelProvider
{ complete(req) }` com adapters anthropic/openai/gemini. Seleção por
`model`/`provider` no agente. Tokens no **vault** com nomenclatura padrão
`PROVIDER_<NOME>_API_KEY` (ex.: `PROVIDER_OPENAI_API_KEY`).

### B4. APIs de serviços externos — schema de `connectors`
Não há conceito estruturado de "integração/conector" — só secrets soltos no
vault. → Definir `connectors.json` versionado:
`{ id, type, config, secret_ref, enabled }` (ex.: `type:"sqs"`,
`config:{queue_url,region}`, `secret_ref:"PROVIDER_AWS_*"`). Um `ConnectorRegistry`
+ port por categoria (mensageria/storage/provider acima). Credenciais sempre via
`secret_ref` → vault (nunca inline no JSON).

### B5. Webhooks / tools — já coberto por MCP
Agentes já consomem MCP servers (canal oficial de tools externas). Documentar
como o ponto de extensão pra "chamar API de serviço X" a partir de um agente.

---

## C) Princípios de evolução (checklist pra toda mudança)

1. **Aditivo primeiro** — novo campo/rota sim; renomear/remover só com migração.
2. **`schema_version` + migração lazy** no read de cada arquivo de estado.
3. **API versionada** — breaking → `/api/v2`, mantém `v1`.
4. **Port antes de adapter** — abstrai em `core-ports`, default local, externo
   é só mais um adapter.
5. **Secrets sempre no vault** via `secret_ref` padronizado — nunca inline.
6. **Eventos versionados** (`type` + `schema_version`) — contrato estável pro
   outbox/SQS/SNS.
7. **Feature flag** pra ligar enforcement/integração novo (como
   `PROJECT_MEM_REQUIRE_HUMAN` / `PROJECT_MEM_GUARD_ORIGIN`) — rollout sem susto.
