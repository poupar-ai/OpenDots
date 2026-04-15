# /mem-search — Busca no histórico do projeto

Busca no histórico de sessões e observações do projeto atual.

**Query:** $ARGUMENTS

## 1. Encontre o diretório de memória

```bash
git rev-parse --show-toplevel
```

O diretório de memória é `<raiz>/.claude-project-memory/`. Se não existir, informe que este projeto ainda não tem histórico e sugira rodar `/mem-setup`.

## 2. Leia os dados

Leia os dois arquivos:
- `.claude-project-memory/sessions.jsonl` — cada linha é um registro de sessão (type: "start" ou "end")
- `.claude-project-memory/observations.jsonl` — cada linha é uma observação (type: "file_edit", "bash", "note")

## 3. Execute a busca

**Se `$ARGUMENTS` estiver vazio:** mostre os últimos 10 registros de cada tipo.

**Se tiver query:**

Busque em todos os campos de texto:
- Sessions (type=end): campo `summary`
- Observations (file_edit): campo `file`
- Observations (bash): campo `cmd`
- Observations (note): campo `content`

A busca é case-insensitive e parcial (substring match).

**Filtros por prefixo na query:**
- `file: <termo>` → busca só em file_edit
- `bash: <termo>` → busca só em bash
- `session: <termo>` → busca só em resumos de sessão
- Sem prefixo → busca em tudo

## 4. Exiba os resultados

Agrupe por tipo, mais recentes primeiro:

```
## Sessões  (N encontradas)
[data]  [resumo]
[data]  [resumo]

## Arquivos editados  (N encontradas)
[data hora]  [caminho do arquivo]
[data hora]  [caminho do arquivo]

## Comandos executados  (N encontradas)
[data hora]  [comando]  → exit [code]
[data hora]  [comando]  → exit [code]

## Notas  (N encontradas)
[data hora]  [conteúdo]
```

Se nenhum resultado: informe claramente e sugira termos alternativos com base no que existe no histórico.

Se muitos resultados (>30): mostre os 30 mais recentes e informe quantos foram omitidos.
