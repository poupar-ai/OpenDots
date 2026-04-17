# claude-project-memory

[![GitHub Sponsors](https://img.shields.io/github/sponsors/schedylabs?label=Sponsor&logo=githubsponsors&color=ea4aaa)](https://github.com/sponsors/schedylabs)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](./LICENSE)

> ⚠️ **Em desenvolvimento ativo** — funcional, mas ainda em evolução. Feedbacks, bugs e PRs são bem-vindos. Abra uma [issue](https://github.com/schedylabs/claude-project-memory/issues) se encontrar algum problema.

**Plugin para [Claude Code](https://claude.ai/code) que mantém memória persistente por projeto**, armazenada como arquivos de texto simples dentro do próprio repositório — versionável no git, legível por humanos, sem serviços externos, sem banco de dados.

Cada projeto tem sua própria memória. O time inteiro compartilha o contexto ao clonar o repo.

---

## Filosofia

O plugin organiza a memória do projeto em **camadas de identidade**, da mais volátil à mais estável:

```
           ┌────────────────────────┐
           │   ✨  SOUL.md          │  ← ALMA: valores, filosofia, voz, não-negociáveis
           │   (raramente muda)     │     — quem o projeto é
           └────────────────────────┘
           ┌────────────────────────┐
           │   PROJECT.md           │  ← IDENTIDADE FACTUAL: stack, estrutura, padrões
           │   (muda por release)   │     — o que o projeto é
           └────────────────────────┘
       ┌───────────────────────────────┐
       │   DECISIONS / GOTCHAS /       │  ← APRENDIZADO: decisões, armadilhas, glossário
       │   GLOSSARY                    │     — o que o projeto escolheu
       └───────────────────────────────┘
     ┌────────────────────────────────────┐
     │   decisions / corrections /        │  ← LOGS: append-only, alimentado por hooks
     │   gotchas (JSONL)                  │     e slash commands
     └────────────────────────────────────┘
   ┌──────────────────────────────────────────┐
   │   observations.jsonl + graph.json        │  ← SINAL BRUTO: edições, diffs, comandos,
   │                                          │     padrões detectados
   └──────────────────────────────────────────┘
```

### As camadas

- **Alma** (`SOUL.md`) — a identidade do projeto: valores, filosofia, voz, anti-padrões. Responde **"no que esse projeto acredita?"**. Raramente muda. É a **lente** pela qual os fatos são interpretados — por isso é injetada antes de qualquer coisa.
- **Identidade factual** (`PROJECT.md`) — stack, arquitetura, padrões, como rodar. Responde **"o que esse projeto é?"**. Muda por release.
- **Aprendizado** (`DECISIONS.md`, `GOTCHAS.md`, `GLOSSARY.md`) — renderizado automaticamente a partir dos JSONLs ao final de cada sessão.
- **Logs curados** (`decisions.jsonl`, `corrections.jsonl`, `gotchas.jsonl`) — fatos pontuais alimentados por hooks (correções detectadas, armadilhas descobertas em `fail→retry→success`) e por slash commands (`/mem-decision`).
- **Sinal bruto** (`observations.jsonl` + `graph.json`) — capturado automaticamente pelos hooks: edições com `git diff --numstat`, comandos com resumo de saída de testes/lint, e padrões derivados (arquivos ativos, acoplamentos, workflows).

### A ideia central

> O Claude não tem que lembrar. Mas também não deveria repetir erros, nem perder o "por quê" das decisões, **nem esquecer quem é o projeto**.

O plugin é o contrato entre sessões — e entre você, seu time e o Claude.

---

## O problema

Toda vez que você abre o Claude Code, ele começa do zero. Não sabe:

- O que foi feito nas sessões anteriores
- Quais padrões e convenções o projeto usa
- Decisões técnicas que foram tomadas e por quê
- O que está em andamento

Isso leva a respostas genéricas, violações de padrões estabelecidos e retrabalho constante para "re-ensinar" o contexto.

## A solução

O **claude-project-memory** mantém um contexto vivo por projeto e o injeta automaticamente no início de cada conversa.

```
Você abre o Claude Code
        ↓
Banner: "🧠 project-man | meu-projeto | 12 sessões · 340 obs | PROJECT.md ✓"
        ↓
Você digita o primeiro prompt
        ↓
Claude já recebe: PROJECT.md + sessões recentes + observações anteriores + padrões do grafo
        ↓
Responde com contexto completo do projeto desde o primeiro message
        ↓
Cada edição e comando é gravado silenciosamente em background
        ↓
Ao encerrar → resumo da sessão gerado automaticamente
```

---

## Instalação

### Via Claude Code (recomendado)

Abra o Claude Code e peça:

> "Instale o plugin schedylabs/claude-project-memory"

Ou execute diretamente no terminal:

```bash
claude plugins marketplace add schedylabs/claude-project-memory
claude plugins install project-mem@project-mem
```

Reinicie o Claude Code. Pronto.

### Manual

**Pré-requisito:** [Claude Code](https://claude.ai/code) + Node.js

```bash
git clone git@github.com:schedylabs/claude-project-memory.git
cd claude-project-memory
make install
```

```bash
make status     # verifica se está instalado
make uninstall  # remove o plugin
```

O `make install` faz merge seguro no `~/.claude/settings.json` sem sobrescrever outras configurações.

---

## Como a memória é armazenada

Dentro de cada projeto, em `.claude-project-memory/`:

```
meu-projeto/
├── .claude/
│   └── CLAUDE.md                     ← gerado automaticamente pelo plugin (leitura nativa do Claude)
└── .claude-project-memory/
    ├── SOUL.md             ← alma: valores, filosofia, voz (raramente muda)
    ├── PROJECT.md          ← contexto permanente (stack, padrões, decisões)
    ├── DECISIONS.md        ← log renderizado de decisões do projeto
    ├── GOTCHAS.md          ← armadilhas detectadas ou registradas
    ├── GLOSSARY.md         ← glossário de termos (se glossary.jsonl existir)
    ├── config.json         ← configurações do projeto (nome, versão, criação)
    ├── sessions.jsonl      ← histórico de sessões com resumos + kind
    ├── observations.jsonl  ← edições (com diff stat) e comandos (com output summary)
    ├── decisions.jsonl     ← decisões registradas via /mem-decision
    ├── corrections.jsonl   ← correções detectadas nas mensagens do dev
    ├── gotchas.jsonl       ← sequências fail→retry→success detectadas
    ├── graph.json          ← grafo de padrões detectados
    ├── stats.json          ← cache de contadores (usado pelo banner)
    └── tmp/                ← flags de sessão temporárias
```

### Como o contexto chega ao Claude

O plugin usa dois mecanismos em paralelo para máxima confiabilidade:

| Mecanismo | Quando | O que injeta |
|-----------|--------|-------------|
| `.claude/CLAUDE.md` nativo | Toda sessão, antes de qualquer hook | PROJECT.md completo |
| Hook `UserPromptSubmit` | Primeiro prompt da sessão | Sessões recentes + observações + grafo |

O `.claude/CLAUDE.md` é atualizado automaticamente toda vez que o PROJECT.md muda. Isso garante que o Claude leia o contexto do projeto **como parte do system prompt**, mesmo sem nenhum hook ativo.

**Por que JSONL?**
- `git diff` mostra exatamente o que mudou na memória entre commits
- `git blame` mostra quando cada informação foi adicionada
- Qualquer editor abre e edita
- Funciona com ferramentas de busca padrão (`grep`, `jq`, `cat`)
- Zero dependência de banco de dados

**Commitar ou não?** Você decide. Por padrão, o plugin não adiciona `.gitignore`. Commitar o `.claude-project-memory/` significa que qualquer dev que clonar o projeto já tem o contexto completo.

---

## Grafo de padrões

Além das observações brutas, o plugin mantém um `graph.json` que cresce com o uso e detecta padrões automaticamente:

| Padrão | O que detecta |
|--------|---------------|
| **Arquivos mais ativos** | Quais arquivos você edita com mais frequência |
| **Acoplamentos** | Pares de arquivos editados juntos na mesma sessão |
| **Workflows** | Sequências recorrentes de "editar arquivo X → rodar comando Y" |

Esses padrões são injetados como contexto adicional no início de cada sessão:

```
## Padrões detectados

### Arquivos mais ativos
- `src/auth.py` — 14x
- `tests/test_auth.py` — 11x

### Acoplamentos (editados juntos)
- `auth.py` ↔ `test_auth.py` (9x)

### Workflows detectados (editar → rodar)
- editar `auth.py` → `pytest tests/ -v` (7x)
```

---

## Skills (comandos)

### `/mem-setup` — Onboarding do projeto

Analisa o projeto em profundidade e cria o `PROJECT.md`.

O que faz:
1. Lê manifestos de dependência, configs, estrutura de pastas e amostra de código
2. Apresenta um diagnóstico: stack, arquitetura, padrões identificados, como rodar
3. Pergunta ao dev por contexto que não está no código (restrições, decisões, armadilhas)
4. Cria `.claude-project-memory/PROJECT.md` com tudo documentado

A partir daí, o Claude nunca "esquece" o padrão do projeto.

```
/mem-setup
```

### `/mem-soul` — Curar a alma do projeto

Abre uma conversa estruturada para criar ou atualizar `SOUL.md` — a camada de identidade do projeto. Valores, filosofia, estética de código, anti-padrões, voz.

```
/mem-soul                             # onboarding completo
/mem-soul add: nunca logar CPF...     # adiciona um ponto
/mem-soul update: Voz                 # atualiza uma seção
```

A alma é injetada no topo do contexto do Claude em toda sessão futura — antes do `PROJECT.md`. Pense nela como a lente pela qual o Claude deve interpretar as decisões do dia-a-dia.

### `/mem-decision <o que> — <por quê>` — Registrar decisão

Registra uma decisão técnica, arquitetural ou de produto. A decisão é append-only em `decisions.jsonl`, rendered em `DECISIONS.md`, e injetada em toda sessão futura.

```
/mem-decision gateway primário será Asaas — tem Pix + cartão 12x + menor taxa
/mem-decision usar UUID v7 como PK: performance + ordenação temporal natural
```

Separadores aceitos: `—`, `--`, `:`, `porque`, `because`. Sem separador, o skill pergunta a motivação antes de salvar.

### `/mem-search <query>` — Busca no histórico

```
/mem-search autenticação
/mem-search pytest
/mem-search "banco de dados"
/mem-search file: models.py
/mem-search bash: docker
/mem-search session: refactor
```

Filtros por prefixo: `file:`, `bash:`, `session:` — sem prefixo busca em tudo.

### `/mem-ui` — Interface web

Sobe uma UI local em `http://localhost:37778` com abas para toda a memória do projeto:

- **✨ Alma** — `SOUL.md` com tratamento visual destacado (é a camada mais importante)
- **PROJECT.md** — identidade factual do projeto
- **Decisões** — cards com `what` + `why` de cada decisão registrada
- **Correções** — mensagens do dev interpretadas como correção, com a ação anterior do Claude como contexto
- **Armadilhas** — gotchas detectadas automaticamente (sequências `fail → edit → success`)
- **Glossário** — termos do projeto, renderizado a partir de `glossary.jsonl`
- **Sessões** — histórico com resumos, contagem de obs e classificação (`kind`)
- **Observações** — todas as edições (com diff stats) e comandos (com output summary), com busca em tempo real

Atualização automática a cada 10 segundos.

```
/mem-ui
```

Para subir manualmente:

```bash
node /caminho/para/claude-project-memory/plugin/ui/server.cjs
```

---

## O que é gravado automaticamente

| Evento | O que grava |
|--------|-------------|
| `Write` / `Edit` / `MultiEdit` | Caminho relativo + `git diff --numstat` (insertions/deletions) + preview das linhas alteradas |
| `Bash` | Comando executado + exit code + resumo de saída (`pytest: 5 passed, 2 failed`, `ruff: ok`, etc.) |
| `UserPromptSubmit` (correção) | Mensagem do dev começando com marcadores de correção (`não`, `pare`, `errado`, `stop`, `wrong`…), com a ação anterior do Claude como contexto |
| `SessionEnd` (gotcha) | Sequências `bash falhou → editar arquivo → mesmo bash passou` viram entradas em `gotchas.jsonl` |
| `SessionEnd` (kind) | Cada sessão recebe classificação: `feature`, `bugfix`, `refactor`, `docs`, `config`, `exploration` |

Comandos triviais são ignorados (`ls`, `pwd`, `echo`, `cat`, `head`, `tail`, etc.).

## O que é injetado no início de cada sessão

O hook `UserPromptSubmit` monta um bloco de contexto com tudo que for relevante (em ordem):

0. `SOUL.md` completo — alma do projeto (lente pela qual tudo é interpretado)
1. `PROJECT.md` completo
2. `DECISIONS.md`, `GOTCHAS.md`, `GLOSSARY.md` (se existirem)
3. Estado do git — branch atual, modificados não commitados, últimos 5 commits
4. ADRs do repo — lê `docs/adr/*.md` (ou `docs/adrs/`, `docs/decisions/`, `adr/`) e injeta título de cada
5. Resumos das últimas 5 sessões (com `kind`)
6. Últimas 5 decisões registradas
7. Últimas 8 correções do dev — para não repetir erros já corrigidos
8. Últimas 5 armadilhas detectadas
9. Últimas 40 observações (edições com diff stat + comandos com output summary)
10. Padrões do grafo (arquivos ativos, acoplamentos, workflows)

---

## Estrutura do repositório

```
claude-project-memory/
├── Makefile                        ← install / uninstall / status
├── scripts/
│   ├── install.cjs                 ← atualiza ~/.claude/settings.json
│   ├── uninstall.cjs
│   └── status.cjs
└── plugin/
    ├── hooks/hooks.json            ← SessionStart, UserPromptSubmit, PostToolUse, SessionEnd
    ├── scripts/
    │   ├── lib.cjs                 ← helpers compartilhados
    │   ├── startup.cjs             ← banner de status ao abrir
    │   ├── session-init.cjs        ← injeta contexto no primeiro prompt
    │   ├── observe.cjs             ← grava edições e comandos (async)
    │   ├── session-end.cjs         ← gera resumo ao encerrar
    │   └── graph.cjs               ← grafo de padrões sobre as observações
    ├── mcp/
    │   └── server.cjs              ← MCP server (mem_context, mem_search, mem_add_note)
    ├── skills/
    │   ├── mem-setup/SKILL.md      ← /mem-setup
    │   ├── mem-soul/SKILL.md       ← /mem-soul
    │   ├── mem-decision/SKILL.md   ← /mem-decision
    │   ├── mem-search/SKILL.md     ← /mem-search
    │   └── mem-ui/SKILL.md         ← /mem-ui
    └── ui/
        ├── server.cjs              ← servidor HTTP sem dependências
        └── index.html              ← frontend single-page
```

---

## Comparação com claude-mem

|  | [claude-mem](https://github.com/thedotmack/claude-mem) | claude-project-memory |
|--|---|---|
| Armazenamento | `~/.claude-mem/` (global na máquina) | `.claude-project-memory/` (dentro de cada projeto) |
| Formato | SQLite + Chroma (binário) | JSONL (texto puro) |
| Git | Não commitável diretamente | Feito para commitar e compartilhar |
| Worker | Processo Bun em background | Sem worker — scripts Node.js diretos |
| Dependências | Bun, tree-sitter, SQLite, Chroma | Apenas Node.js |
| Busca | Semântica via embeddings | Full-text + grafo de padrões |
| Compartilhamento | Memória pessoal de cada dev | Compartilhada via git com o time |

Os dois podem coexistir — servem propósitos diferentes.

---

## Contribuindo

Issues, PRs e sugestões são bem-vindos.

> Se você usa **Claude Code** para contribuir com este projeto, o repositório já tem um `CLAUDE.md` na raiz com contexto completo da arquitetura, convenções e como testar. O Claude lê esse arquivo automaticamente ao abrir o projeto.

```bash
git clone git@github.com:schedylabs/claude-project-memory.git
cd claude-project-memory
```

Não há build step. Os scripts são Node.js CJS, editáveis diretamente.

**Para testar um hook manualmente:**

```bash
# banner de status
node plugin/scripts/startup.cjs

# injeção de contexto
echo '{"session_id":"test","cwd":"/seu/projeto"}' | node plugin/scripts/session-init.cjs

# gravar observação
echo '{"session_id":"test","tool_name":"Bash","tool_input":{"command":"pytest -v"},"tool_response":{"exit_code":0}}' | node plugin/scripts/observe.cjs
```

**Áreas abertas para contribuição:**
- Perfis de stack (Python/FastAPI, Go, Next.js, Laravel, etc.) para análise especializada no `/mem-setup`
- Suporte a monorepos
- Exportação da memória para Markdown
- Integração com `git log` para enriquecer o contexto automaticamente

---

## Apoie o projeto

O **claude-project-memory** é gratuito e open source. Se ele economiza tempo no seu trabalho, considere apoiar o desenvolvimento:

[![GitHub Sponsors](https://img.shields.io/badge/Sponsor-%E2%9D%A4-ea4aaa?logo=githubsponsors)](https://github.com/sponsors/schedylabs)

**Roadmap pago (em planejamento):**

| Feature | Plano |
|---------|-------|
| Sync da memória entre máquinas e membros do time | Pro |
| Dashboard cloud (sem precisar rodar `localhost`) | Pro |
| Busca semântica sobre as observações | Pro |
| Perfis de stack prontos (FastAPI, Go, Next.js...) | Pro |
| Suporte prioritário | Pro |

Se você tem interesse nas features Pro ou quer discutir suporte para sua equipe: **maisbitlabs@gmail.com**

---

## Palavras-chave

`claude-code` `claude-code-plugin` `ai-memory` `project-context` `developer-tools` `llm-context` `persistent-memory` `git-friendly` `claude-ai` `ai-assistant` `coding-assistant` `context-injection` `knowledge-graph`

---

## Licença

MIT — veja [LICENSE](./LICENSE).
