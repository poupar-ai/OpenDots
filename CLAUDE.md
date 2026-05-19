# claude-project-memory — Contexto para Claude Code

## O que é este projeto

Plugin para Claude Code que mantém memória persistente por projeto. Armazena contexto, histórico de sessões e observações em arquivos de texto simples (JSONL) dentro do próprio repositório — versionável no git, sem dependências externas.

## Stack

- Node.js (CJS, sem build step, sem dependências externas)
- Hooks do Claude Code: `SessionStart`, `UserPromptSubmit`, `PostToolUse`, `SessionEnd`
- MCP server via stdio (JSON-RPC 2.0)

## Estrutura de diretórios

```
plugin/
  hooks/hooks.json       ← definição dos 4 hooks
  scripts/
    lib.cjs              ← helpers compartilhados (ÚNICA fonte de verdade para caminhos)
    startup.cjs          ← SessionStart → banner de status
    session-init.cjs     ← UserPromptSubmit → injeta contexto uma vez por sessão
    observe.cjs          ← PostToolUse → grava observação + atualiza grafo
    session-end.cjs      ← SessionEnd → gera resumo da sessão
    graph.cjs            ← grafo de padrões sobre as observações
  mcp/server.cjs         ← MCP server com 4 tools
  skills/                ← /mem-setup, /mem-search, /mem-ui
  ui/                    ← servidor HTTP + frontend single-page
scripts/
  install.cjs            ← atualiza ~/.claude/settings.json
  uninstall.cjs
  status.cjs
```

## Convenções críticas

- **Nunca use `path.join(x, '.claude-project-memory')` diretamente** — sempre use `getMemDir(cwd)` de `lib.cjs`. O nome do diretório é a constante `MEM_DIR_NAME` em `lib.cjs`.
- **Flag files de sessão ficam em `tmp/`** — `getTmpDir(memDir)` de `lib.cjs`.
- **Para ler a última observação, use `readLastJsonlLine(file)`** — não leia o arquivo inteiro.
- **Para contar sessões/observações no banner, use `loadStats(memDir)`** — não leia os JSONL.
- **Ao criar PROJECT.md, chame `syncClaudeMd(projectRoot, content)`** para sincronizar `.claude/CLAUDE.md`.
- **Todos os scripts são Node.js CJS** — não use ESM, não use `import`.
- **Zero dependências externas** — se precisar de algo, implemente ou use módulos built-in do Node.js.

## UI (ui-next) — arquitetura hexagonal

- **Panels SEMPRE importam de `@/ui`**, nunca de `@mantine/core` direto. `@/ui` é a porta única; o adapter atual é `src/ui/adapter-mantine/`.
- **Novo componente / novo panel?** Usa `@/ui`. Se faltar algum primitivo, adiciona o wrapper em `adapter-mantine/`, re-exporta em `adapter-mantine/index.ts`, e usa.
- **Editando um arquivo que ainda usa Tailwind ou `@/components/ui/*`?** Migra ele pra `@/ui` no mesmo commit. Assim a migração avança organicamente — em vez de uma "fase 3" gigante, cada toque vira progresso.
- **Cores/tamanhos** são semânticos: `color="primary|success|warning|danger|info|gray"`, `size="xs|sm|md|lg"`. Nada de `color="indigo"` no panel — isso é detalhe do Mantine, vive no adapter.
- **Precisa de cor inline (border/bg)?** Usa `colorVar(color, shade)` de `@/ui`. Evita `var(--mantine-color-*)` solto.
- **Trocar de lib UI no futuro**: cria `src/ui/adapter-<nome>/` espelhando a mesma API pública, troca a linha de re-export em `src/ui/index.ts`, ajusta `MantineProvider` em `main.tsx`. Panels não mudam.

## Backend (api-http) — TenantContext

- **Handlers acessam `mem_dir` via `Extension<TenantContext>`**, nunca `st.mem_dir` direto. Scaffold em `src/tenant.rs` + `src/auth.rs` (middleware). Hoje sempre devolve tenant `default` — comportamento idêntico ao pré-tenant.
- **Padrão pra novo handler**:
  ```rust
  use axum::extract::Extension;
  use crate::tenant::TenantContext;

  async fn meu_handler(
      Extension(tc): Extension<TenantContext>,
      body: Bytes,
  ) -> Response {
      // usa tc.mem_dir em vez de st.mem_dir
      // se também precisar de outras coisas do AppState (project_root, port, etc.):
      //   State(st): State<AppState>,
  }
  ```
- **Migração de handlers existentes**: feita organicamente — quando tocar um handler que ainda usa `st.mem_dir`, migra ele no mesmo commit. Exemplos já migrados: todo `scope_routes.rs`, `humans_routes::create_handler`.
- **Por que isso importa**: quando multi-tenancy ativar de verdade, basta editar `resolve_tenant` em `auth.rs` (lookup token → tenant_id em DB). Zero refactor de handler. Sem esse cuidado, cada novo handler com `st.mem_dir` direto vira mais uma futura refatoração arriscada.

## Como testar manualmente

```bash
# Banner de status
node plugin/scripts/startup.cjs

# Injeção de contexto no primeiro prompt
echo '{"session_id":"test","cwd":"/seu/projeto"}' | node plugin/scripts/session-init.cjs

# Gravar observação
echo '{"session_id":"test","tool_name":"Bash","tool_input":{"command":"pytest -v"},"tool_response":{"exit_code":0},"cwd":"/seu/projeto"}' | node plugin/scripts/observe.cjs

# MCP server — handshake
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | node plugin/mcp/server.cjs

# MCP server — listar tools
printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}\n{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}\n' | node plugin/mcp/server.cjs
```

## Fluxo de dados

```
SessionStart     → startup.cjs      → systemMessage (banner)
UserPromptSubmit → session-init.cjs → additionalContext (PROJECT.md + sessões + obs + grafo)
PostToolUse      → observe.cjs      → observations.jsonl + graph.json + stats.json
SessionEnd       → session-end.cjs  → sessions.jsonl (resumo) + limpa tmp/
```

## Estrutura de memória por projeto

```
<raiz-do-projeto>/
├── .claude/
│   └── CLAUDE.md                       ← sincronizado de PROJECT.md (leitura nativa do Claude)
└── .claude-project-memory/
    ├── PROJECT.md                       ← contexto permanente do projeto
    ├── config.json                      ← metadados (project, version, created_at)
    ├── sessions.jsonl                   ← {type:"start"|"end", session_id, ...}
    ├── observations.jsonl               ← {type:"file_edit"|"bash"|"note", session_id, ...}
    ├── graph.json                       ← {nodes:{}, edges:{}}
    ├── stats.json                       ← {sessions:N, observations:N}
    └── tmp/
        └── .init-{session_id}           ← flag: contexto já injetado nesta sessão
```
