---
name: mem-soul
description: Cria ou atualiza .poupar/SOUL.md — camada de identidade do projeto (valores, filosofia, voz, não-negociáveis).
---

# /mem-soul — Alma do projeto

Cria ou atualiza `.poupar/SOUL.md`, que é a **camada de identidade** do projeto — valores, filosofia, voz e não-negociáveis que vão além do que é factual (stack, estrutura, decisões pontuais).

**Entrada:** $ARGUMENTS — opcional. Se vazio, inicia o onboarding completo. Se for `add: <texto>` ou `update: <seção>`, atualiza pontualmente.

## Alma × Memória

| Camada | O quê | Frequência de mudança |
|--------|-------|----------------------|
| **Alma** (`SOUL.md`) | Valores, filosofia, estética, voz, anti-padrões | Raríssima. Muda por amadurecimento, não por tarefa. |
| **Memória factual** (`PROJECT.md` + JSONLs) | Stack, estrutura, decisões pontuais, correções, armadilhas | Contínua. Cresce a cada sessão. |

A alma responde **"no que esse projeto acredita?"**. A memória responde **"o que esse projeto fez?"**. O Claude lê ambas, mas a alma vem primeiro — ela é a lente pela qual os fatos são interpretados.

## 1. Encontre a raiz

```bash
git rev-parse --show-toplevel
```

O arquivo é `<raiz>/.poupar/SOUL.md`.

## 2. Se `$ARGUMENTS` está vazio — onboarding da alma

**Antes de perguntar, leia o que já existe:**

- `.poupar/PROJECT.md` (se existir — dá contexto factual)
- `README.md` da raiz
- Arquivos de negócio/plano se existirem (ex: `business/`, `docs/vision.md`, `docs/product.md`)

Depois, conduza uma **conversa estruturada** com o dev. Não despeje todas as perguntas de uma vez — faça uma de cada vez, aprofunde quando a resposta for rasa.

**Perguntas essenciais (nessa ordem):**

1. **Propósito** — "Por que esse projeto existe? Em uma frase, pra quem e pra resolver o quê?"
2. **Valores não-negociáveis** — "Quais são 3-5 coisas que esse projeto **nunca** vai sacrificar, mesmo quando difícil?" (exemplos pra destravar: privacidade do usuário, simplicidade do código, taxa zero para X, prazo de resposta, qualidade sobre velocidade)
3. **Filosofia de produto** — "Como vocês enxergam o usuário? O que acreditam sobre o mercado/problema que outros concorrentes erram?"
4. **Estética de código** — "Se um dev novo entrar, que tipo de código vocês querem que ele escreva? O que te irrita ver em PRs?"
5. **Anti-padrões** — "O que é comum no mercado mas vocês recusam fazer?" (ex: dark patterns, over-engineering, dependências de moda, docs só depois)
6. **Voz e tom** — "Como vocês falam com usuário? Direto, formal, brincalhão, técnico? Exemplos?"

**Regras da conversa:**

- Se a resposta for genérica ("código limpo", "boa UX"), pergunte de novo com um exemplo concreto: "me dá um caso real em que isso foi testado?"
- Se o dev não sabe uma resposta, **não invente**. Escreva `_A definir_` na seção.
- Captura fiel: transcreva as palavras do dev, não traduza para seu jargão.

## 3. Crie/atualize `SOUL.md`

**Estrutura:**

```markdown
# Alma do projeto <nome>
> A identidade do projeto. Mude apenas com reflexão — esta é a camada mais estável da memória.
> Atualizado em: <data>

## Propósito
<uma frase direta>

## Valores não-negociáveis
- **<nome curto do valor>** — <explicação em 1-2 frases>
- ...

## Filosofia de produto
<parágrafo fluido; capturar a visão de mundo, não só features>

## Estética de código
<lista ou parágrafo; o que o time considera código "bom" aqui>

## Anti-padrões (o que recusamos)
- **<anti-padrão>** — <por quê>
- ...

## Voz
<tom + 2-3 exemplos curtos de frases que representam o jeito>
```

**Regras:**
- Seções vazias recebem `_A definir_` — não inventar conteúdo
- Nada de frases genéricas estilo "buscamos excelência"
- Se já existe um SOUL.md, **não sobrescreva** cegamente. Mostre o diff das mudanças propostas e confirme com o dev antes de salvar.

## 4. Se `$ARGUMENTS` começa com `add:` — adicionar ponto

Exemplo: `/mem-soul add: nunca logar CPF em texto plano — valor de privacidade do usuário`

Identifique qual seção o ponto pertence (na dúvida, pergunte), faça append e confirme.

## 5. Se `$ARGUMENTS` começa com `update:` — atualizar seção

Exemplo: `/mem-soul update: Voz`

Leia a seção atual, mostre ao dev, pergunte o que mudar, edite.

## 6. Ao final

- Mostre o conteúdo final do `SOUL.md` ao dev
- Sugira commit: `git add .poupar/SOUL.md && git commit -m "docs: atualiza alma do projeto"`
- Informe: "A partir da próxima sessão, a alma é injetada no topo do contexto do Claude — antes do PROJECT.md."

## Não confundir com

- **`/mem-decision`** — decisões pontuais (escolher biblioteca X, arquitetura Y). Ficam em `decisions.jsonl`, não na alma.
- **`/mem-setup`** — onboarding factual (stack, estrutura, como rodar). Escreve `PROJECT.md`, não a alma.

Regra prática: **se pode mudar na próxima sprint, é decisão. Se define quem o projeto é, é alma.**
