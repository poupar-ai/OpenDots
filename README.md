# claude-project-memory

[![GitHub Sponsors](https://img.shields.io/github/sponsors/schedylabs?label=Sponsor&logo=githubsponsors&color=ea4aaa)](https://github.com/sponsors/schedylabs)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](./LICENSE)

> ⚠️ **Em desenvolvimento ativo** — funcional, mas ainda em evolução. Feedbacks, bugs e PRs são bem-vindos. Abra uma [issue](https://github.com/schedylabs/claude-project-memory/issues) se encontrar algum problema.

**Plugin para [Claude Code](https://claude.ai/code) que mantém memória persistente por projeto**, armazenada como arquivos de texto simples dentro do próprio repositório — versionável no git, legível por humanos, sem serviços externos, sem banco de dados.

Cada projeto tem sua própria memória. O time inteiro compartilha o contexto ao clonar o repo.

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

**Pré-requisito:** [Claude Code](https://claude.ai/code) + Node.js

```bash
git clone git@github.com:schedylabs/claude-project-memory.git
cd claude-project-memory
make install
```

Reinicie o Claude Code. Pronto.

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
    ├── PROJECT.md          ← contexto permanente (stack, padrões, decisões)
    ├── config.json         ← configurações do projeto (nome, versão, criação)
    ├── sessions.jsonl      ← histórico de sessões com resumos
    ├── observations.jsonl  ← edições e comandos registrados
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

Sobe uma UI local em `http://localhost:37778` com:
- Visualizador do `PROJECT.md`
- Histórico de sessões com datas e resumos
- Todas as observações com busca em tempo real
- Atualização automática a cada 10 segundos

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
| `Write` / `Edit` / `MultiEdit` | Caminho relativo do arquivo editado |
| `Bash` | Comando executado + exit code |

Comandos triviais são ignorados (`ls`, `pwd`, `echo`, `cat`, `head`, `tail`, etc.).

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
