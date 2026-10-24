# Instalar qualquer pacote do GitHub como ferramenta dos agentes

Qualquer repositório público do GitHub pode virar uma ferramenta (servidor MCP stdio)
que os seus agentes usam. Não precisa publicar nada no npm, PyPI ou crates.io: o
OpenDots baixa o repositório **fixado num commit**, detecta o tipo do projeto e registra
o comando que sobe o MCP. Você escolhe quais agentes recebem a ferramenta.

## Pela UI

1. Abra a UI (`poupar-http`, porta 37778) e clique em **mcps** no topo.
2. Clique em **do GitHub**.
3. Cole a URL do repositório. Formatos aceitos:
   - `https://github.com/dono/repo`
   - `github.com/dono/repo`
   - `dono/repo`
   - `https://github.com/dono/repo/tree/<branch|tag|commit>` para fixar uma referência.
4. Clique em **analisar**. Isso baixa o repo, detecta o tipo e mostra o comando que
   será usado, sem instalar nada ainda.
5. Marque quais agentes podem usar a ferramenta.
6. Clique em **instalar**.

Pronto: a ferramenta aparece no catálogo de MCPs do projeto e os agentes marcados
passam a ter `mcp__<nome>` liberado no `--allowed-tools`. Os demais agentes continuam
sem acesso.

## Pela API

```bash
curl -X POST http://127.0.0.1:37778/api/tools/github \
  -H 'content-type: application/json' \
  -d '{ "url": "https://github.com/dono/repo", "agents": ["pesquisador", "dev"] }'
```

Campos do body:

| Campo | Obrigatório | O que faz |
|---|---|---|
| `url` | sim | URL do repositório (ver formatos acima) |
| `ref` | não | branch, tag ou commit. Sobrepõe o `/tree/...` da URL. Sem isso usa o commit atual da branch padrão |
| `name` | não | nome da ferramenta (`[a-z0-9_-]`, até 64). Padrão: nome do repo em minúsculas |
| `agents` | não | lista de nomes de agentes, ou `"*"` para todos. Sem isso a ferramenta entra no catálogo mas ninguém recebe |
| `command` / `args` | só para repo sem manifesto reconhecido | comando que sobe o MCP. `{repo}` vira o caminho do checkout |
| `env` | não | variáveis de ambiente para o processo do MCP |
| `dry_run` | não | `true` só baixa e detecta, devolvendo a definição sem instalar nem registrar. É o **analisar** da UI |

## Como cada tipo de projeto é detectado

| O repo tem | Como a ferramenta roda |
|---|---|
| `lightpanda-io/browser` | baixa o binário da release e roda `lightpanda mcp` (navegador headless com 30+ ferramentas: `goto`, `markdown`, `click`, `fill`, `screenshot`…) |
| `package.json` com `bin` | `npx -y --package=github:dono/repo#<sha> <bin>` |
| monorepo (`packages/*`) | `npx -y --package=<pacote>@<versão> <bin>` |
| `pyproject.toml` com `[project.scripts]` | `uvx --from git+https://github.com/dono/repo@<sha> <script>` |
| `Cargo.toml` | `cargo install --git … --rev <sha>` e roda o binário |
| `go.mod` | `go run <módulo>@<sha>` |
| nenhum dos acima | você informa `command` e `args`; `{repo}` vira o caminho do código baixado |

A rota nunca executa shell vindo do request. O comando só roda quando o Claude Code sobe
o MCP, como qualquer outro MCP do catálogo.

## Onde as coisas ficam

- Binários e checkouts: `$POUPAR_TOOLS_DIR` ou
  `${XDG_DATA_HOME:-~/.local/share}/open-poupar/tools/<dono>__<repo>/<sha>`. Fora do
  repositório do projeto, porque binário de terceiro não deve entrar no git.
- Catálogo de MCPs do projeto: `.poupar/mcps.json`, materializado em `.mcp.json`.
- Permissão por agente: campo `mcps[]` de cada agente em `.poupar/agents.json`.

## Atualizar ou remover

Atualizar é registrar de novo com outra `ref` (ou sem `ref`, para pegar o commit atual).
Nada atualiza sozinho. Para remover, apague a ferramenta no painel **mcps**; isso tira a
entrada do catálogo e do `mcps[]` dos agentes.

## Exemplo real: Lightpanda como navegador do pesquisador

```bash
curl -X POST http://127.0.0.1:37778/api/tools/github \
  -H 'content-type: application/json' \
  -d '{ "url": "https://github.com/lightpanda-io/browser", "agents": ["pesquisador"] }'
```

Depois disso o `pesquisador` abre páginas de verdade; um agente sem o Lightpanda recebe
permissão negada ao tentar.

> **Atenção:** uma ferramenta roda com as permissões do seu usuário. Instale só repositórios
> em que você confia.
