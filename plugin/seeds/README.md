# Seeds

Agentes-template organizados por **categoria de negócio** (tech, atendimento, delivery, vendas...) e **pack** dentro da categoria.

```
seeds/
├── <categoria>/
│   ├── meta.json              # label + descrição da categoria
│   └── <pack>/
│       ├── meta.json          # label + descrição do pack
│       ├── agents.json        # manifest (subset de .claude-project-memory/agents.json)
│       └── agents/*.md        # mini-contexto de cada agente
```

## Uso

```bash
# wizard interativo (recomendado no primeiro uso)
node plugin/scripts/seed.cjs setup

# listar categorias e packs disponíveis
node plugin/scripts/seed.cjs --list

# aplicar um pack sem wizard
node plugin/scripts/seed.cjs tech/team
node plugin/scripts/seed.cjs tech/team --project /caminho
node plugin/scripts/seed.cjs tech/team --only manager,oracle,architect
node plugin/scripts/seed.cjs tech/team --exclude asaas,email_sender
node plugin/scripts/seed.cjs tech/team --overwrite
```

Sem `--overwrite`, é idempotente: só adiciona o que ainda não existe.

## Categorias atuais

- **tech** — Negócio de tecnologia (engenharia, SaaS, produto digital)
  - `tech/team` — time completo de produto (17 agentes)

## Adicionando novas categorias/packs

1. Criar `seeds/<categoria>/meta.json` com `{ "label": "...", "description": "..." }`.
2. Criar `seeds/<categoria>/<pack>/meta.json` igual.
3. Criar `seeds/<categoria>/<pack>/agents.json` (mesmo formato do `.claude-project-memory/agents.json`).
4. Criar `seeds/<categoria>/<pack>/agents/<nome>.md` para cada agente.
5. Pronto — aparece automaticamente no `--list` e no wizard.
