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

## A) Riscos de compatibilidade (resolver ANTES de ter usuários reais)

### A1. API sem versionamento — **alto**
Não há `/api/vN`. Qualquer mudança de shape quebra clientes (UI embarcada hoje,
mas integrações externas no futuro). → Adotar **`/api/v1`** (ou header
`X-API-Version`) e fixar contrato: **mudanças só aditivas** dentro de uma versão;
breaking → nova versão, antiga mantida por um período.

### A2. Versionamento de dados ad-hoc — **alto**
Hoje cada arquivo versiona por conta própria (graph.json `version:2`,
`config.json`, autonomy `version:1`) e nem todos têm. → Padronizar
**`schema_version`** em todo arquivo de estado (humans.json, agents.json,
manifest, observations) + **migração lazy no read** (upgrade no load, como o
vault já faz no unlock). Regra: **nunca renomear/remover campo sem migração**;
adicionar campo é sempre seguro (readers ignoram desconhecidos).

### A3. Contrato dos JSONL — **médio**
JSONL append-only é naturalmente aditivo, mas falta o contrato escrito.
→ Documentar "campos são aditivos; um reader nunca exige campo novo". Marcar
campos deprecados (sem apagar) por uma janela.

### A4. Token/JWT sem versão — **médio**
Claims do JWT (humano/agente) não têm `ver`. Mudar o formato/algoritmo
invalidaria tokens em uso. → Adicionar `ver` nos claims + plano de rotação de
chave (regenerar keypair invalida JWTs — `agents_routes.rs:105` já alerta).

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
