# /mem-search — Busca no histórico do projeto

Busca no histórico de sessões e observações do projeto atual.

**Query:** $ARGUMENTS

## 1. Encontre o diretório de memória

```bash
git rev-parse --show-toplevel
```

O diretório de memória é `<raiz>/.claude-project-memory/`. Se não existir, informe que este projeto ainda não tem histórico e sugira rodar `/mem-setup`.

## 2. Leia os dados

Leia os arquivos que existirem:
- `.claude-project-memory/sessions.jsonl` — registros de sessão (type: "start" ou "end", pode ter `kind`)
- `.claude-project-memory/observations.jsonl` — observações (type: "file_edit", "bash", "note"; file_edit pode ter `diff`; bash pode ter `output_summary`)
- `.claude-project-memory/decisions.jsonl` — decisões registradas (`what`, `why`)
- `.claude-project-memory/corrections.jsonl` — correções detectadas (`prompt`, `previous_action`)
- `.claude-project-memory/gotchas.jsonl` — armadilhas (`description`, `file`, `command`, `failure_hint`)

## 3. Execute a busca

**Se `$ARGUMENTS` estiver vazio:** mostre os últimos 10 registros de cada tipo.

**Se tiver query:**

Busque em todos os campos de texto:
- Sessions (type=end): `summary`, `kind`
- Observations (file_edit): `file`, `diff.preview`
- Observations (bash): `cmd`, `output_summary`
- Observations (note): `content`
- Decisions: `what`, `why`
- Corrections: `prompt`, `previous_action.file`, `previous_action.cmd`
- Gotchas: `description`, `file`, `command`, `failure_hint`

A busca é case-insensitive e parcial (substring match).

**Filtros por prefixo na query:**
- `file: <termo>` → busca só em file_edit
- `bash: <termo>` → busca só em bash
- `session: <termo>` → busca só em resumos de sessão
- `decision: <termo>` → busca só em decisions.jsonl
- `correction: <termo>` → busca só em corrections.jsonl
- `gotcha: <termo>` → busca só em gotchas.jsonl
- Sem prefixo → busca em tudo

## 4. Exiba os resultados

Agrupe por tipo, mais recentes primeiro:

```
## Sessões  (N encontradas)
[data] [kind]  [resumo]

## Arquivos editados  (N encontradas)
[data hora]  [caminho do arquivo]  (+N/-N)

## Comandos executados  (N encontradas)
[data hora]  [comando]  → exit [code]  [output_summary]

## Decisões  (N encontradas)
[data]  [what] — [why]

## Correções  (N encontradas)
[data]  após [previous_action]: "[prompt]"

## Armadilhas  (N encontradas)
[data]  [file]: [description]

## Notas  (N encontradas)
[data hora]  [conteúdo]
```

Se nenhum resultado: informe claramente e sugira termos alternativos com base no que existe no histórico.

Se muitos resultados (>30): mostre os 30 mais recentes e informe quantos foram omitidos.
