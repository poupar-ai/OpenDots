---
name: mem-index
description: Rebuild do code index do projeto — mapa de símbolos reutilizáveis (funções, classes, endpoints, tabelas) com ranking por importância.
---

# /mem-index — Code index do projeto

Faz rebuild completo do **code index** — o mapa de símbolos reutilizáveis (funções, classes, endpoints, tabelas) do projeto. Estratégia estilo Aider/Cursor/Sourcegraph: extração por linguagem + contagem de referências + ranking por importância.

**Entrada:** `$ARGUMENTS` — opcional. Sem argumento faz rebuild. Com `show` mostra o `CODE_INDEX.md` atual.

## Quando usar

- Primeira vez configurando o index neste projeto
- Quando adicionar uma nova linguagem / framework
- Suspeita que o index está desatualizado (normalmente não precisa — o session-end atualiza incrementalmente)
- Debug: quer ver estatísticas e top símbolos

> Durante uso normal, o index é atualizado automaticamente quando uma sessão termina (só pra arquivos editados naquela sessão). `/mem-index` força rebuild completo.

## 1. Rebuild

```bash
_R="${CLAUDE_PLUGIN_ROOT:-/home/tiago/pessoal/project-mem/plugin}"
node -e "
  const { rebuildIndex } = require('$_R/scripts/indexer.cjs');
  const path = require('path');
  const { execSync } = require('child_process');
  const root = execSync('git rev-parse --show-toplevel').toString().trim();
  const memDir = path.join(root, '.claude-project-memory');
  const start = Date.now();
  const idx = rebuildIndex(memDir, root);
  const files = Object.keys(idx.files).length;
  const syms  = Object.values(idx.files).reduce((n, f) => n + f.symbols.length, 0);
  console.log('files:', files, '· símbolos:', syms, '· em', Date.now()-start, 'ms');
"
```

Gera dois arquivos em `.claude-project-memory/`:
- `code_index.json` — dados brutos consultáveis
- `CODE_INDEX.md` — renderização legível (top símbolos, arquivos mais densos)

## 2. Show — exibir o index atual

Quando `$ARGUMENTS` = `show`:

```bash
cat "$(git rev-parse --show-toplevel)/.claude-project-memory/CODE_INDEX.md"
```

## 3. Como é usado em sessões futuras

O `session-init.cjs` injeta automaticamente a seção **"Code Index (símbolos reutilizáveis)"** no contexto — só dos **arquivos mais quentes** (top 10 do grafo de edições), limitado a 25 símbolos. Isso evita estourar o orçamento de tokens em projetos grandes.

## 4. Linguagens suportadas

- **Python** — `def`, `async def`, `class`, rotas FastAPI (`@router.get/post/...`)
- **TypeScript/JavaScript** — `export function/const/class/interface/type/enum`, rotas Next App Router
- **Rust** — `fn`, `pub fn`, `struct`, `enum`, `trait`
- **SQL** — `CREATE TABLE`, `CREATE TYPE`, `CREATE INDEX`
- **Go** — `func`, `type X struct/interface`

Extração é por regex (plugin dependency-free). Cobre >95% dos símbolos que importam pra ranking — pra dúvidas raras o Claude grep no arquivo real.

## 5. Ranking (estilo Aider)

```
score = hot_file_weight × 0.5     (arquivos mais editados pesam mais)
      + log2(1 + refs)   × 0.3   (símbolos mais referenciados pesam mais)
      + recency_decay    × 0.2    (arquivos recentes pesam mais, meia-vida 30d)
```

`hot_file_weight` vem do `graph.json` (grafo de co-edições). `refs` vem de scan regex por nome em todo o repo.

## Não confundir com

- **`/mem-search`** — busca em observações, decisões, correções. O code index é sobre **código**, não sobre histórico.
- **`/mem-setup`** — cria o `PROJECT.md`. O code index é alimentado automaticamente, sem intervenção manual normal.
