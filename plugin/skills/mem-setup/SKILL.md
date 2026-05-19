---
name: mem-setup
description: Analisa o projeto em profundidade e cria/atualiza .claude-project-memory/PROJECT.md pra manter consistência em sessões futuras.
---

# /mem-setup — Onboarding do projeto

Você vai analisar este projeto em profundidade e criar (ou atualizar) o arquivo `.claude-project-memory/PROJECT.md`, que será injetado em toda sessão futura para manter consistência total com os padrões do projeto.

## 1. Encontre a raiz do projeto

```bash
git rev-parse --show-toplevel
```

Toda operação de leitura e escrita usa essa raiz.

## 2. Analise o projeto sistematicamente

Execute os passos abaixo em ordem. Não pule nenhum.

**Estrutura geral:**
```bash
ls -la
ls src/ app/ lib/ internal/ cmd/ 2>/dev/null
```

**Manifesto de dependências** — leia o que existir:
- `package.json` / `package-lock.json`
- `pyproject.toml` / `requirements.txt` / `setup.py`
- `go.mod`
- `Cargo.toml`
- `pom.xml` / `build.gradle`
- `composer.json`

**Configuração e infraestrutura** — leia o que existir:
- `README.md`
- `.env.example` / `.env.sample`
- `docker-compose.yml` / `Dockerfile`
- `Makefile`
- `.github/workflows/`
- Arquivos de config do framework (ex: `next.config.js`, `vite.config.ts`, `alembic.ini`)

**Amostra de código** — leia 3 a 5 arquivos representativos:
- Um arquivo de entrada/main (ex: `main.py`, `index.ts`, `app.go`)
- Um model ou schema
- Um controller ou handler
- Um arquivo de teste se existir

## 3. Apresente o diagnóstico ao desenvolvedor

Antes de criar o arquivo, apresente:
- Nome e propósito do projeto (1-2 frases)
- Stack completa com versões principais
- Estrutura de diretórios comentada
- Padrões de código identificados (naming, organização, estilo)
- Como rodar, testar e fazer build
- Pontos que ficaram vagos ou precisam de confirmação

Depois pergunte:

> "Encontrei isso na análise. Há convenções, restrições técnicas, decisões de arquitetura ou contexto de negócio que você quer que eu sempre leve em conta neste projeto — e que não estão evidentes no código?"

Aguarde a resposta antes de criar o arquivo.

## 4. Crie `.claude-project-memory/PROJECT.md`

Use exatamente esta estrutura:

```markdown
# [nome do projeto]
> Atualizado em: [data]

## O que é
[1-3 frases diretas sobre o propósito do projeto]

## Stack
[linguagem principal + versão]
[framework principal + versão]
[dependências críticas que impactam decisões de código]

## Estrutura de diretórios
[pasta/] — [o que contém e por quê]
(apenas as pastas relevantes, não todas)

## Arquitetura e padrões
[como o código é organizado: MVC, clean arch, hexagonal, etc.]
[padrões de nomeação: snake_case, camelCase, etc.]
[convenções de arquivo: um componente por arquivo, etc.]

## Como rodar
[comandos exatos de instalação, dev, test, build]

## Restrições e decisões importantes
[coisas que o Claude nunca deve mudar sem discussão]
[decisões técnicas já tomadas e por quê]
[armadilhas conhecidas do projeto]

## Contexto adicional do desenvolvedor
[o que o dev disse na etapa 3 — transcreva fielmente, não resuma]
```

**Regras:**
- Seja direto e específico — sem texto genérico que vale para qualquer projeto
- A seção "Restrições" deve ter pelo menos 2-3 itens concretos ou ficar vazia (não invente)
- "Contexto do desenvolvedor" só recebe o que o dev disse explicitamente

## 5. Confirme e sincronize

Após criar o PROJECT.md:

1. Informe ao dev onde o arquivo foi criado e o que foi capturado em cada seção.
2. Sugira commitar: `git add .claude-project-memory/PROJECT.md && git commit -m "docs: add project memory"`
3. O plugin vai sincronizar automaticamente o conteúdo para `.claude/CLAUDE.md` na próxima sessão, garantindo que o Claude leia o contexto nativamente.
