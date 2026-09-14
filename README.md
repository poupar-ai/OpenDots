# open-poupar.ai

**A edição aberta da [poupar.ai](https://poupar.ai) — memória persistente e um time de
agentes para o [Claude Code](https://claude.ai/code), dentro do seu repositório.**

[![poupar.ai](https://img.shields.io/badge/poupar.ai-site-2F6FED)](https://poupar.ai)
[![License: PolyForm Noncommercial](https://img.shields.io/badge/License-PolyForm%20Noncommercial%201.0.0-orange.svg)](./LICENSE)
[![GitHub Sponsors](https://img.shields.io/github/sponsors/schedylabs?label=Sponsor&logo=githubsponsors&color=ea4aaa)](https://github.com/sponsors/schedylabs)

![O time de agentes do open-poupar.ai trabalhando](docs/assets/demo-agentes.gif)

## O que faz

Toda vez que você abre o Claude Code, ele começa do zero. O open-poupar.ai resolve isso
com duas coisas que funcionam juntas:

**1. Memória que mora no repositório.** Cada edição, comando e decisão vira uma linha de
JSONL dentro de `.claude-project-memory/`, no seu próprio projeto. No começo de cada
sessão isso é injetado de volta no contexto do Claude — ele já abre sabendo a stack, os
padrões, as decisões e as armadilhas do projeto. Como é texto puro versionado no git,
`git diff` mostra o que a memória aprendeu, e quem clonar o repo herda o contexto inteiro.

**2. Um time de agentes, não um assistente só.** Você define papéis — manager, oracle,
arquiteto, revisor, tester, devs por senioridade, especialistas — cada um com seu modelo,
suas ferramentas, seu escopo e seu contexto. Eles conversam entre si, delegam, escalam
para você quando travam, e tudo fica registrado. O canvas acima é a UI local onde você
acompanha e conversa com o time.

Concretamente, ele:

| | |
|---|---|
| **Grava** | edições (com `git diff --numstat`), comandos (com resumo de saída de teste/lint), correções suas e armadilhas `falha → conserto → sucesso` |
| **Injeta** | SOUL/PROJECT/DECISIONS/GOTCHAS, sessões recentes, estado do git, índice de código e padrões do grafo — com orçamento de tokens por prioridade |
| **Detecta** | arquivos mais quentes, acoplamentos (arquivos editados juntos) e workflows recorrentes (`editar X → rodar Y`) |
| **Orquestra** | time de agentes com handoff, subagentes paralelos, cron, pipelines e fila com retry |
| **Protege** | cofre criptografado por projeto, identidade Ed25519 por agente, allowlist de comandos e bloqueio de operação destrutiva |

Roda inteiro na sua máquina: sem conta, sem nuvem, sem telemetria. O binário é estático e
não depende de runtime nenhum.

---

## Instalação

> **Licença:** uso livre para fins **não-comerciais** (pessoal, estudo, ONG,
> escola, órgão público). Uso em empresa exige licença comercial — veja
> [Licença](#licença).

### Via curl (recomendado)

**Pré-requisito:** [Claude Code](https://claude.ai/code). Só isso — os binários
são compilados e distribuídos prontos; não precisa de Node, Rust nem git.

```bash
curl -fsSL https://raw.githubusercontent.com/poupar-ai/open-poupar.ai/main/install.sh | bash
```

O instalador detecta seu SO/arquitetura, baixa o tarball da última release,
**confere o SHA-256**, instala em `~/.local/share/project-mem`, cria o symlink
`~/.local/bin/pmem` e registra o plugin no `~/.claude/settings.json` (merge
seguro — não sobrescreve suas outras configurações).

Reinicie o Claude Code e rode `/mem-setup` dentro de um projeto.

Plataformas com binário pronto:

| SO | Arquitetura | Target |
|---|---|---|
| Linux | x86_64 | `x86_64-unknown-linux-musl` (estático) |
| Linux | ARM64 | `aarch64-unknown-linux-musl` (estático) |
| macOS | Intel | `x86_64-apple-darwin` |
| macOS | Apple Silicon | `aarch64-apple-darwin` |

Ajustes por variável de ambiente:

```bash
# versão específica em vez da última
curl -fsSL .../install.sh | PMEM_VERSION=v0.1.0 bash

# outro diretório de instalação
curl -fsSL .../install.sh | PMEM_HOME=/opt/project-mem bash

# instalar os arquivos sem tocar no settings.json
curl -fsSL .../install.sh | PMEM_NO_REGISTER=1 bash
```

Para desinstalar (a memória dos seus projetos **não** é tocada):

```bash
curl -fsSL https://raw.githubusercontent.com/poupar-ai/open-poupar.ai/main/uninstall.sh | bash
```

Prefere não canalizar um script pro shell? Baixe, leia, execute:

```bash
curl -fsSLO https://raw.githubusercontent.com/poupar-ai/open-poupar.ai/main/install.sh
less install.sh && bash install.sh
```

### Via Claude Code

Abra o Claude Code e peça:

> "Instale o plugin poupar-ai/open-poupar.ai"

Ou execute diretamente no terminal:

```bash
claude plugins marketplace add poupar-ai/open-poupar.ai
claude plugins install project-mem@project-mem
```

Reinicie o Claude Code. Pronto.

### Do código-fonte

**Pré-requisitos:** [Claude Code](https://claude.ai/code) + Rust 1.88 + Node.js
(a UI é buildada com Vite).

```bash
git clone https://github.com/poupar-ai/open-poupar.ai.git
cd open-poupar.ai
make install
```

```bash
make status     # verifica se está instalado
make uninstall  # remove o plugin
```

Na instalação por clone os hooks rodam a implementação `.cjs` (Node); no
tarball rodam os binários compilados. Os dois produzem a mesma saída — quem
decide é o `plugin/hooks/run-hook.sh`, que só escolhe o binário quando ele
está presente em `plugin/bin/`.

Para gerar seu próprio tarball de release:

```bash
scripts/package.sh                              # target nativo
scripts/package.sh --target aarch64-apple-darwin  # cross-compile
scripts/smoke-package.sh                        # confere que o tarball roda
```

### E o Node?

O runtime do plugin — hooks, MCP server, servidor HTTP e UI — é 100% Rust
compilado e não precisa de Node. Duas coisas opcionais ainda chamam Node
quando você as usa: os skills `/mem-agents` e `/mem-index`, e o CLI de seed
(`plugin/scripts/seed.cjs setup`). Sem Node instalado o plugin funciona; só
esses extras ficam indisponíveis.

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

O **open-poupar.ai** mantém um contexto vivo por projeto e o injeta automaticamente no início de cada conversa.

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
# instalação por tarball
PROJECT_MEM_PORT=37778 ~/.local/share/project-mem/plugin/bin/pmem-http

# instalação por clone (precisa de build prévio)
cd /caminho/para/open-poupar.ai
cargo build --release --manifest-path v2/Cargo.toml -p api-http
cd ui-next && npm install && npm run build
PROJECT_MEM_PORT=37778 v2/target/release/pmem-http
```

O binário deve ser executado a partir da raiz do projeto que você quer
inspecionar — é o `cwd` que define qual memória a UI mostra.

---

## Contribuindo um skill novo

Os skills do projeto seguem o formato **[agentskills.io](https://agentskills.io/specification)** — open standard da Anthropic adotado por Codex, Cursor, GitHub Copilot, Gemini CLI, JetBrains Junie, Goose e outros. Isso significa que um skill escrito aqui é portável: tools que respeitam o spec carregam ele direto.

### Layout

```
plugin/skills/<slug>/
├── SKILL.md            # obrigatório — frontmatter YAML + corpo markdown
├── scripts/            # opcional — executáveis (Python/Bash/JS)
├── references/         # opcional — docs auxiliares carregados sob demanda
└── assets/             # opcional — arquivos estáticos (templates, etc)
```

### `SKILL.md`

Começa com frontmatter YAML obrigatório:

```markdown
---
name: meu-skill              # ≤64 chars, mesmo slug da pasta
description: O que faz, em uma linha — aparece na descoberta.
---

# /meu-skill — Título legível

Instruções, exemplos, passos pro Claude executar.
```

Campos:
- **`name`** (obrigatório, ≤64 chars) — slug do skill, idêntico ao nome da pasta. Usado como `/<name>` no Claude Code.
- **`description`** (obrigatório) — uma linha; é o que o modelo vê na fase de descoberta (~100 tokens).

### Progressive disclosure

Pense em 3 camadas conforme o agente avança:

1. **Descoberta** (~100 tokens) — só o frontmatter. O modelo decide se invoca o skill com base na `description`.
2. **Ativação** (<5k tokens) — corpo do `SKILL.md`. Instruções, exemplos, passos. Não dilua com referências enormes.
3. **Recursos sob demanda** — `scripts/`, `references/`, `assets/` só carregam quando o corpo do SKILL.md instrui (`leia references/X.md`, `rode scripts/Y.py`).

### Workflow

1. Crie a pasta: `plugin/skills/<slug>/`
2. Escreva `SKILL.md` com frontmatter + corpo
3. (Opcional) Adicione `scripts/`, `references/`, `assets/` se precisar
4. Reinstale o plugin (`make install` ou reabra Claude Code) — skill vira `/<slug>` automaticamente

### Não tente expandir o padrão pra outras camadas

`agentskills.io` cobre **só a camada de skills**. Identidade de agente (`agents.json`), config MCP (`.mcp.json`), runtime de handoff inter-agente (JWT Ed25519 + queue persistente) seguem proprietários — não há padrão aberto consolidado pra essas camadas ainda. Se quiser estender o sistema nesses pontos, fica no formato deste projeto.

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
open-poupar.ai/
├── install.sh                      ← instalador curl | bash
├── uninstall.sh
├── Makefile                        ← install / uninstall / status (fonte)
├── scripts/
│   ├── package.sh                  ← monta o tarball de release
│   ├── smoke-package.sh            ← verifica que o tarball roda fora do clone
│   ├── install.cjs                 ← atualiza ~/.claude/settings.json
│   ├── uninstall.cjs
│   └── status.cjs
├── plugin/
│   ├── bin-path.sh                 ← resolve onde os binários compilados estão
│   ├── hooks/
│   │   ├── hooks.json              ← UserPromptSubmit, PostToolUse, SessionEnd
│   │   └── run-hook.sh             ← escolhe binário compilado ou .cjs
│   ├── scripts/*.cjs               ← implementação Node (instalação por clone)
│   ├── mcp/server.cjs              ← MCP server em Node (instalação por clone)
│   ├── skills/                     ← /mem-setup, /mem-soul, /mem-decision, …
│   ├── agent-bin/pmem              ← wrapper de API usado dentro dos agentes
│   └── seeds/ pipelines/ best-practices/
├── ui-next/                        ← front React + Vite (build → ui-next/dist)
└── v2/                             ← workspace Rust (a implementação compilada)
    ├── crates/api-cli              → pmem
    ├── crates/api-hooks            → pmem-hook-{startup,observe,session-init,session-end}
    ├── crates/api-http             → pmem-http (API + UI)
    ├── crates/api-mcp              → pmem-mcp
    ├── crates/core-*               ← paths, storage, queue, ports, branding
    └── crates/domain-*             ← 20+ domínios (agents, graph, vault, cron, …)
```

Layout de uma instalação por tarball — é isso que o `install.sh` deixa em
`~/.local/share/project-mem`:

```
project-mem/
├── .claude-plugin/marketplace.json ← faz o Claude Code reconhecer o plugin
├── plugin/
│   ├── bin/                        ← os 7 binários compilados
│   └── hooks/ skills/ seeds/ …     ← assets do plugin
├── ui/                             ← build da UI
└── VERSION
```

Os binários acham `plugin/` e `ui/` a partir do **próprio executável**
(`core_paths::install`), subindo até o `.claude-plugin/marketplace.json`.
É o que permite instalar em qualquer diretório sem variável de ambiente.

---

## Onde ele se diferencia

O espaço de "memória para agente de código" tem três famílias, e elas resolvem problemas
diferentes. Vale entender em qual você está entrando antes de escolher.

**Serviços hospedados** (Supermemory, Mem0 e afins, inclusive como provider do Hermes
Agent) — memória fica num serviço externo, com busca semântica e perfil que atravessa
projetos e ferramentas. Você ganha recall entre contextos; paga com dependência de rede,
conta e um terceiro guardando o que seu time discute.

**Plugins de memória local** (do `MEMORY.md` curado ao SQLite com embeddings) — arquivo ou
banco na sua máquina, injetado no início da sessão. Simples e privado; a memória costuma
ser pessoal daquela máquina e o escopo é lembrar fatos.

**open-poupar.ai** — memória versionada *dentro do repositório* **e** um time de agentes
operando sobre ela.

O que decorre disso, concretamente:

| | open-poupar.ai |
|---|---|
| **Onde a memória vive** | `.claude-project-memory/` no seu repo — `git diff` mostra o que ela aprendeu, `git blame` mostra quando, e quem clona herda o contexto |
| **De quem é a memória** | do projeto, não da máquina: o time inteiro compartilha pelo git, sem sync nem servidor |
| **Formato** | JSONL e Markdown legíveis — `grep`, `jq` e qualquer editor funcionam; nada de blob binário |
| **Escopo** | além de lembrar fatos: time de agentes com papéis, handoff, subagentes paralelos, cron, pipelines e fila com retry |
| **Dependências** | binário estático — sem conta, sem nuvem, sem telemetria, sem runtime |
| **Se o projeto sumir** | a memória continua legível: são arquivos de texto no seu repo, sem formato proprietário |

O custo honesto dessa escolha: a memória **não** atravessa projetos nem máquinas
automaticamente (é justamente o que o git resolve, e o que um serviço hospedado faz por
você), e a busca é full-text com BM25 (SQLite FTS5) mais um grafo de padrões — não
embeddings semânticos. Se o que você quer é um perfil único te seguindo por todas as
ferramentas, um serviço hospedado atende melhor. Se você quer que **o repositório** saiba
de si mesmo e que um time opere sobre ele, é aqui.

Os dois modelos coexistem — resolvem camadas diferentes.

## Débitos técnicos a resolver

Pontos mapeados pra melhorar (com referência ao código). Ordenados por impacto.

> Planejamento de longo prazo:
> - [`docs/evolucao-e-integracoes.md`](docs/evolucao-e-integracoes.md) —
>   compatibilidade (não quebrar quem já usa) + ports pra integrar SQS/SNS,
>   Google Drive/S3, multi-provider de IA, conectores de serviços.
> - [`docs/permissoes-granulares.md`](docs/permissoes-granulares.md) — como
>   evoluir o `authorize()` pra RBAC/ABAC.

### Segurança (auditoria 2026-05-20)

> Postura padrão hoje: `PROJECT_MEM_REQUIRE_HUMAN` on (escritas exigem JWT), mas
> `authorize()` libera tudo a qualquer principal autenticado; **GETs não exigem
> auth**; `REQUIRE_TOKEN` off; bind padrão `lan` (`0.0.0.0`). Confirmado SEGURO:
> sem command-injection no spawn (argv, sem shell), sem SSRF (não há cliente HTTP
> de saída), sem XSS (react-markdown sem `rehype-raw`), `alg=none` impossível.

**Crítico**
- **`authorize()` é no-op** (`api-http/src/auth.rs:89-91`): só checa
  `is_authenticated()`. Qualquer JWT (humano *ou agente*) chama toda rota mutável
  (`/api/db/restore`, `/api/scope`, `/api/mcps`, `/api/agents/:n/vault`…). É o
  amplificador dos demais. → autorização real por principal/recurso; barrar
  agente de mutar manifesto/scope/MCP.
- **`PUT /api/scope` + `POST /api/mcps` → `.mcp.json` em dir arbitrário → RCE**
  (`scope_routes.rs:89-127`, `mcp_routes.rs:151-178`, `:311+`): `project_root`
  sem allowlist + MCP `stdio` com `command` arbitrário; o Claude Code auto-executa
  o `.mcp.json` materializado. → allowlist de `project_root`; tratar `command`
  stdio como privilegiado.
- **Auto-escalada do agente**: agente spawna com `Bash`+`curl *`
  (`chat_send_routes.rs:157-167`), origin-guard libera `curl` (sem `Origin`,
  `auth.rs:160`), `authorize()` libera tudo, e **`get_secrets_for_agent` ignora o
  unlock do cofre** (`adapter-vault-file/src/lib.rs:555-583`) → agente faz
  `curl` pra própria API, se concede todos os secrets e exfiltra no próximo spawn.
- **Frontend vaza Bearer cross-origin** (`ui-next/src/lib/apiAuth.ts:23`):
  anexa o token a *qualquer* URL contendo a substring `/api/` (inclui
  `https://evil.com/api/x`). → ancorar em `window.location.origin` +
  `pathname.startsWith('/api/')` (fix de 1 linha).

**Alto**
- **`is_public_write` por sufixo/prefixo** (`auth.rs:96-102`): `ends_with("/login")`
  e `starts_with("/api/auth/server-token")`. → match exato / guard por-rota.
- **`bash_allow` aceita `*`** sem validação (`domain-agents/src/scope.rs`
  `sanitize_scope`); a denylist global (`chat_send_routes.rs:74-108`) não é
  fronteira. → validar padrões contra gramática allowlist.
- **Injeção de env perigoso** (`chat_send_routes.rs:210-260`, `:963/:965`): secret
  chamado `LD_PRELOAD`/`PATH`/`NODE_OPTIONS` no `secrets` do agente sobrescreve o
  env do servidor. → denylist de nomes reservados em `build_agent_env`.
- **Path traversal em GETs não autenticados**: `agent` em `/api/chat/history` e
  `/stream` (`chat_routes.rs:113+`) e log (`agents_routes.rs:928`); `id` em
  `/api/pipelines/install` (`pipelines_routes.rs:67`) e `ref` em `/api/seeds/apply`
  (`seeds_routes.rs:93`). → aplicar o regex `^[a-z0-9_-]+$` que os GET-by-id já usam.

**Médio (segurança)**
- **`storage=db` grava secret literal em texto plano** no `.mcp.json` (raiz do
  projeto, 0644, fora do `.gitignore`) e em `mcps.json` (`mcp_routes.rs:121-178`)
  → risco de commit. Bloquear literal com `storage=db` (forçar `vault`), escrever
  0600, gerenciar gitignore.
- **"Cofre trancado" enganoso**: device key em texto plano, senha não deriva a
  chave, `change_password` não re-cifra, `get_secrets_for_agent` ignora lock
  (`adapter-vault-file/src/lib.rs`).
- **Secrets de MCP vault-backed exibidos em texto plano** no `McpForm.tsx:91` (via
  `getMcp`) — sem máscara/reveal-gate.
- **Plugin Node**: `mem_dir` guiado por `cwd`/`file_path` não-confiáveis do hook
  (`plugin/scripts/lib.cjs:13-29`, `observe.cjs:147`).
- **Restore não revalida `entry.path` do manifest** (`db_routes.rs:271`) —
  zip-slip latente (vira live com upload).
- **Server token com PRNG fraco** (`auth_routes.rs` `simple_uuid_v4`) + compare
  não-constante (`auth.rs:188`) — relevante só com `REQUIRE_TOKEN=1`.

**Baixo / info**
- `exp` sem teto e claim `ver` não validado; EdDSA usa `verify` (não
  `verify_strict`); JWT humano/agente sem claim `typ`/`aud` (confusão de realm).
- PBKDF2 100k rounds (OWASP ≥600k); senha mínima 4 chars.
- `/api/_dev/mint_token` auto-provisiona identidade — só com `PROJECT_MEM_DEV_MODE=1`.
- Garantir `agent_keys/` e `human_keys/` (`.priv`) no `.gitignore`.
- Prompt-injection via observações reinjetadas; MCP server carrega JSONL inteiro
  (DoS local); `install.cjs` não-atômico; colisão de chave de cofre `foo-bar`↔`foo_bar`.

### Médio

- **9 painéis ainda no legado `@/components/ui/*`** (migrar pra `@/ui`):
  `AgentInfoPanel` (1607 linhas), `VaultPanel`, `PipelinesPanel`,
  `ArchitectPanel`, `MyChatsPanel`, `ProtectionsPanel`, `IdeExportPanel`,
  `BestPracticesPanel`, `CopyButton`.
- **Zero testes de frontend** (sem vitest/jest). Cobrir as partes frágeis:
  reset de agente no `useChat`, lazy-load por janela, `useMediaQuery`, gate do
  cofre.
- **Chat humano por polling (5s), agente por SSE.** `HumansPanel` usa
  `setInterval(refreshChat, 5000)`. → Reusar o `/stream` SSE no chat humano.

### Menor / pendências conhecidas

- **Shadow-turn** (humano offline responder via IA) — `humans_routes.rs`
  ("fica pra depois").
- **Limpar conversa** zera o arquivo do par (ambos os lados) — sem limpeza
  por-usuário (marcador `cleared_at` por humano).
- **~22 handlers** ainda em `st.mem_dir` vs `Extension<TenantContext>` — dívida
  pra quando multi-tenancy ligar.
- **Token do humano em `sessionStorage`** (exposição a XSS) — ok pra local,
  repensar se exposto.
- **Senha do humano exige cofre destrancado** (hash mora no cofre) — gate
  obrigatório hoje; reavaliar quando integrar auth externo (ex.: Firebase).
- **Snapshot/restore**: upload via multipart é "fase futura"
  (`db_routes.rs`).

---

## Contribuindo

Issues, PRs e sugestões são bem-vindos.

> Se você usa **Claude Code** para contribuir com este projeto, o repositório já tem um `CLAUDE.md` na raiz com contexto completo da arquitetura, convenções e como testar. O Claude lê esse arquivo automaticamente ao abrir o projeto.

```bash
git clone https://github.com/poupar-ai/open-poupar.ai.git
cd open-poupar.ai
```

O runtime oficial é o workspace Rust em `v2/` — `cargo build --release` a
partir de `v2/` produz os 7 binários. Os `.cjs` em `plugin/scripts/` são a
implementação Node original, mantida como fallback da instalação por clone
e como referência de paridade dos testes.

**Para testar um hook manualmente:**

```bash
# banner de status
node plugin/scripts/startup.cjs

# injeção de contexto
echo '{"session_id":"test","cwd":"/seu/projeto"}' | node plugin/scripts/session-init.cjs

# gravar observação
echo '{"session_id":"test","tool_name":"Bash","tool_input":{"command":"pytest -v"},"tool_response":{"exit_code":0}}' | node plugin/scripts/observe.cjs
```

**Para testar os binários compilados:**

```bash
cd v2 && cargo build --release && cd ..

# o dispatcher só escolhe o binário quando ele está "instalado" —
# no clone, aponte explicitamente:
export PROJECT_MEM_BIN_DIR="$PWD/v2/target/release"
echo '{"session_id":"test","cwd":"/seu/projeto"}' \
  | CLAUDE_PLUGIN_ROOT="$PWD/plugin" bash plugin/hooks/run-hook.sh session-init
```

### Publicando uma release

1. Suba a `version` em `v2/Cargo.toml` (`[workspace.package]`).
2. Commit, tag e push:

   ```bash
   git tag v0.2.0 && git push origin v0.2.0
   ```

O workflow [`release.yml`](.github/workflows/release.yml) valida que a tag bate
com a `version` do workspace, builda a UI uma vez, compila os 4 targets, gera
`.tar.gz` + `.sha256` de cada um e publica tudo na release. A partir daí o
`install.sh` já serve a versão nova — ele sempre resolve a última release.

Antes de taggear, dá pra ensaiar tudo localmente:

```bash
scripts/package.sh && scripts/smoke-package.sh
```

**Áreas abertas para contribuição:**
- Perfis de stack (Python/FastAPI, Go, Next.js, Laravel, etc.) para análise especializada no `/mem-setup`
- Suporte a monorepos
- Exportação da memória para Markdown
- Integração com `git log` para enriquecer o contexto automaticamente
- Zerar a dívida de `cargo fmt` / `cargo clippy` herdada da migração Node→Rust
  (hoje roda como `continue-on-error` no CI)

---

## Apoie o projeto

O **open-poupar.ai** é gratuito para uso não-comercial e tem o código aberto para leitura, estudo e contribuição. Se ele economiza tempo no seu trabalho, considere apoiar o desenvolvimento:

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

`claude-code` `claude-code-plugin` `poupar-ai` `agent-team` `ai-memory` `project-context` `developer-tools` `llm-context` `persistent-memory` `git-friendly` `claude-ai` `ai-assistant` `coding-assistant` `context-injection` `knowledge-graph`

---

## Licença

[**PolyForm Noncommercial 1.0.0**](https://polyformproject.org/licenses/noncommercial/1.0.0/) — veja [LICENSE](./LICENSE).

O código é aberto para ler, estudar, modificar e redistribuir, mas **só pode
ser usado para fins não-comerciais**. Na prática:

| Uso | Permitido |
|---|---|
| Projetos pessoais, hobby, estudo, pesquisa | ✅ |
| ONGs, escolas, universidades, órgãos públicos, saúde | ✅ |
| Fork, modificação e redistribuição (mantendo a licença e o aviso) | ✅ |
| Usar no trabalho de uma empresa, mesmo internamente | ❌ |
| Vender, hospedar como serviço ou embutir num produto pago | ❌ |

Formalmente isso **não** é uma licença open source pela definição da
[OSI](https://opensource.org/osd) — que exige não discriminar campo de atuação.
O termo correto é *source available*. O código está todo aqui, é auditável e
aceita contribuições; o que a licença restringe é o uso comercial.

**Precisa usar comercialmente?** Existe licença comercial — escreva para
**maisbitlabs@gmail.com**.

Contribuições são bem-vindas sob os mesmos termos: ao abrir um PR você licencia
sua contribuição para o projeto sob a PolyForm Noncommercial 1.0.0.
