---
name: mem-decision
description: Registra uma decisão técnica, arquitetural ou de produto em .claude-project-memory/decisions.jsonl, injetada em sessões futuras.
---

# /mem-decision — Registrar uma decisão do projeto

Registra uma decisão técnica, arquitetural ou de produto no histórico do projeto. Decisões ficam em `.claude-project-memory/decisions.jsonl` e são injetadas automaticamente em toda sessão futura.

**Entrada:** $ARGUMENTS

## 1. Encontre a raiz do projeto

```bash
git rev-parse --show-toplevel
```

Se o comando falhar, use `pwd`. O diretório de memória é `<raiz>/.claude-project-memory/`.

Se o diretório não existir, informe ao dev que este projeto ainda não tem memória e sugira rodar `/mem-setup` antes.

## 2. Parseie `$ARGUMENTS`

Padrões aceitos (do mais estruturado para o mais livre):

1. `<o que> — <por quê>` — separador em travessão longo
2. `<o que> -- <por quê>` — dois hífens
3. `<o que>: <por quê>` — dois pontos
4. `<o que> porque <por quê>` / `<o que> because <por quê>`
5. Texto livre — salva tudo no campo `what`, deixa `why` vazio

Extraia:
- **what** (o que foi decidido) — obrigatório
- **why** (motivação) — opcional mas fortemente recomendado

Se o dev mandar só `$ARGUMENTS` sem explicação clara, pergunte uma vez: **"Qual a motivação dessa decisão?"** e só depois salve. Sem motivação, a decisão perde 80% do valor pro Claude futuro.

## 3. Append no JSONL

Acrescente uma linha em `.claude-project-memory/decisions.jsonl`:

```json
{"id":"<uuid-curto>","what":"<o que>","why":"<por quê>","ts":"<ISO8601>","session_id":"<id-atual se disponível>"}
```

Para o `id`, use um identificador curto e único — pode ser os primeiros 8 chars de um `uuidgen`, ou o timestamp compactado (`YYYYMMDDHHMMSS`). Não invente UUID "na mão".

Comando sugerido:
```bash
printf '%s\n' "$(jq -cn --arg what "<what>" --arg why "<why>" --arg ts "$(date -u +%FT%TZ)" --arg id "$(date -u +%Y%m%d%H%M%S)" '{id:$id, what:$what, why:$why, ts:$ts}')" >> .claude-project-memory/decisions.jsonl
```

Se `jq` não estiver disponível, escreva manualmente a linha JSON tomando cuidado com escape de aspas.

## 4. Renderize DECISIONS.md

Após o append, regenere `.claude-project-memory/DECISIONS.md` a partir do JSONL — simples markdown append-only, ordem cronológica decrescente:

```markdown
# Decisões do projeto

## YYYY-MM-DD — <what>
<why>

## YYYY-MM-DD — <what>
<why>
```

Você pode ler o JSONL completo, mapear cada linha e regravar o MD inteiro. É um arquivo pequeno — não otimize.

## 5. Confirme ao dev

Mostre ao dev:
- A entrada exata que foi gravada (`what` + `why`)
- O caminho do arquivo JSONL e do MD
- Sugestão de commit: `git add .claude-project-memory/ && git commit -m "chore: registrar decisão — <resumo curto>"`

## Exemplos de uso

```
/mem-decision gateway primário será Asaas — tem Pix + cartão 12x + menor taxa
/mem-decision usar UUID v7 como PK: performance + ordenação temporal natural
/mem-decision 0% de comissão por venda porque o diferencial competitivo do produto é exatamente não cobrar por transação
```
