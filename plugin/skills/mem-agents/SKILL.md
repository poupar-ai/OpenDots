---
name: mem-agents
description: Declara os papéis de agentes do projeto (manager, oracle, especialistas) e injeta a config no contexto pro Claude Code usar subagents nativos.
---

# /mem-agents — Registry de agentes do projeto

Declara os papéis de agentes deste projeto (Manager, Oracle, e especialistas). O plugin injeta essa config no contexto pra que o Claude Code use subagents nativos (via Task tool) de forma consistente — com modelo certo, contexto certo e regras de comunicação claras.

**Entrada:** `$ARGUMENTS` — opcional.
- vazio → lista os agentes declarados
- `init` → cria defaults (Manager, Oracle, Architect, Reviewer, Tester, Designer)
- `init --force` → sobrescreve existentes
- `show <nome>` → mostra mini-contexto de um agente
- `add <nome>` → cria novo agente **specialist** (conhecimento interno)
- `add integrator <nome>` → cria **integrator** (conhece API externa, **informa, não age**)
- `add motor <nome>` → cria **motor** (age no mundo real via MCPs, requer confirmação do Manager)
- `add firefighter <nome>` → cria **bombeiro** (escuta logs, dispara triage, escala ao Manager)
- `edit <nome>` → edita um agente existente
- `remove <nome>` → remove um agente (nunca remove Manager ou Oracle sem confirmação)

## Tipos de agente

| Tipo | O quê faz | Exemplos |
|------|-----------|----------|
| `core` | Manager (🎯) e Oracle (🔮) — sistema | manager, oracle |
| `specialist` | Conhecimento interno do projeto | architect, reviewer, tester, designer |
| `integrator` | Conhece API externa a fundo — **INFORMA, não age** | asaas, firebase, bigdatacorp |
| `motor` | **Age no mundo real** via MCPs/rede — com guardrails | email_sender, whatsapp_sender, slack_notifier |
| `firefighter` | **Reativo** — escuta logs, dispara triage, escala ao Manager | api_firefighter, payments_firefighter |

Integradores não têm MCPs. Motores sempre requerem confirmação do Manager antes de agir. Bombeiros só diagnosticam — correção passa por Manager + motor.

## 1. Prepare o acesso

O manifesto de agentes é manipulado pela API local do `poupar-http`, não
por edição direta do JSON — a API valida, versiona e escreve o log de
auditoria junto.

```bash
ROOT=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
MEM="$ROOT/.poupar"
API="http://localhost:${POUPAR_PORT:-37778}"

# a UI/servidor precisa estar no ar
curl -sf "$API/api/data" >/dev/null || echo "servidor parado — rode /mem-ui primeiro"
```

## 2. Se `$ARGUMENTS` está vazio — listar agentes

```bash
curl -sf "$API/api/data" | python3 -c '
import json, sys
d = json.load(sys.stdin)["agents"]
m = d.get("manifest") or {}
ag = m.get("agents") or {}
if not ag:
    print("nenhum agente declarado. Rode /mem-agents init pra criar os defaults.")
    raise SystemExit
counts, recent = d.get("counts") or {}, d.get("recent") or {}
print("Manager:", m.get("manager"), "| Oracle:", m.get("oracle"))
print("Regra:", m.get("rule") or "(nenhuma)")
print()
for name, a in ag.items():
    last = f" · último uso {recent[name][:10]}" if recent.get(name) else ""
    print("•", a.get("emoji",""), name, "—", a.get("role",""))
    print("    modelo:", a.get("model"), "| invocações:", counts.get(name,0), last, sep=" ")
'
```

## 3. Se `$ARGUMENTS` = `init`

Mostra os defaults que serão criados, pergunta confirmação, então chama:

```bash
curl -sf -X POST "$API/api/agents" \
  -H 'Content-Type: application/json' \
  -d @- <<'JSON'
{ "name": "<nome>", "type": "<specialist|integrator|motor|firefighter>",
  "emoji": "<emoji>", "role": "<uma linha>", "model": "<modelo>",
  "tools": ["Read","Grep","Glob"], "can_talk_to": ["manager"] }
JSON
```

O corpo aceita os mesmos campos do manifesto (seção 8). Depois de criar,
grave o mini-contexto do agente:

```bash
curl -sf -X PUT "$API/api/agents/<nome>/context" \
  -H 'Content-Type: text/plain' --data-binary @- <<'MD'
<conteúdo do agents/<nome>.md>
MD
```

Informe o dev:
- Papéis criados (Manager 🎯, Oracle 🔮, Architect 🏛, Reviewer 🔍, Tester 🧪, Designer 🎨)
- Cada um tem mini-contexto em `.poupar/agents/<nome>.md` — pode editar pra personalizar pro projeto
- A partir da próxima sessão, o bloco **"Agentes do projeto"** é injetado no contexto do Claude

**Com `--force`:** sobrescreve mini-contextos existentes também. Pergunte antes.

## 4. Se `$ARGUMENTS` = `show <nome>`

```bash
NAME="<nome>"
cat "$MEM/agents/$NAME.md"
```

## 5. Se `$ARGUMENTS` = `add <nome>` (specialist)

Conduza uma conversa curta:

1. "Qual o papel desse agente em uma frase?"
2. "Qual modelo? (opus / sonnet / haiku)"
3. "Quais ferramentas ele usa? (Read, Write, Edit, Bash, Grep, Glob, Task — escolha as relevantes)"
4. "Ele fala com quais outros agentes?"
5. "Emoji representativo? (opcional)"

Depois chame:

```bash
curl -sf -X POST "$API/api/agents" \
  -H 'Content-Type: application/json' \
  -d @- <<'JSON'
{ "name": "<nome>", "type": "<specialist|integrator|motor|firefighter>",
  "emoji": "<emoji>", "role": "<uma linha>", "model": "<modelo>",
  "tools": ["Read","Grep","Glob"], "can_talk_to": ["manager"] }
JSON
```

O corpo aceita os mesmos campos do manifesto (seção 8). Depois de criar,
grave o mini-contexto do agente:

```bash
curl -sf -X PUT "$API/api/agents/<nome>/context" \
  -H 'Content-Type: text/plain' --data-binary @- <<'MD'
<conteúdo do agents/<nome>.md>
MD
```

## 5a. Se `$ARGUMENTS` = `add integrator <nome>`

Um **integrator** conhece uma API externa a fundo. **Informa, não age** — quem age é o motor correspondente. Nunca guarde credenciais: só referencie env vars pelo nome.

Conduza:

1. "Qual serviço este integrator representa? (ex: Asaas, Stripe, Firebase)"
2. "URL da documentação oficial?"
3. "Base URL da API?"
4. "Tipo de autenticação? (API key / OAuth / JWT / header customizado)"
5. "Env vars esperadas (só os NOMES, nunca valores)?"
6. "Principais endpoints que este agente deve conhecer? (método + path + propósito)"
7. "Webhooks relevantes? (opcional)"
8. "Emoji? (padrão: 🔌)"

Depois chame:

```bash
curl -sf -X POST "$API/api/agents" \
  -H 'Content-Type: application/json' \
  -d @- <<'JSON'
{ "name": "<nome>", "type": "<specialist|integrator|motor|firefighter>",
  "emoji": "<emoji>", "role": "<uma linha>", "model": "<modelo>",
  "tools": ["Read","Grep","Glob"], "can_talk_to": ["manager"] }
JSON
```

O corpo aceita os mesmos campos do manifesto (seção 8). Depois de criar,
grave o mini-contexto do agente:

```bash
curl -sf -X PUT "$API/api/agents/<nome>/context" \
  -H 'Content-Type: text/plain' --data-binary @- <<'MD'
<conteúdo do agents/<nome>.md>
MD
```

## 5b. Se `$ARGUMENTS` = `add motor <nome>`

Um **motor** age no mundo real (envia email, manda mensagem no WhatsApp, faz chamada HTTP que muda estado externo). Requer MCPs configurados no Claude Code e confirmação do Manager.

Conduza:

1. "Qual ação este motor executa? (ex: enviar email, postar no WhatsApp, criar cobrança no Asaas)"
2. "Quais MCP servers ele precisa? (precisam estar configurados em `~/.claude/settings.json` — o plugin só referencia pelo nome)"
3. "Ferramentas permitidas? (Read, Write, Bash, Task, MCPs...)"
4. "Env vars necessárias (só NOMES)?"
5. "Quais ações são permitidas?"
6. "Quais ações são proibidas? (ex: envio em massa, dados sensíveis em texto plano)"
7. "Toda ação precisa de aprovação explícita do Manager? (padrão: sim)"
8. "Emoji? (padrão: ⚙️)"

Depois chame:

```bash
curl -sf -X POST "$API/api/agents" \
  -H 'Content-Type: application/json' \
  -d @- <<'JSON'
{ "name": "<nome>", "type": "<specialist|integrator|motor|firefighter>",
  "emoji": "<emoji>", "role": "<uma linha>", "model": "<modelo>",
  "tools": ["Read","Grep","Glob"], "can_talk_to": ["manager"] }
JSON
```

O corpo aceita os mesmos campos do manifesto (seção 8). Depois de criar,
grave o mini-contexto do agente:

```bash
curl -sf -X PUT "$API/api/agents/<nome>/context" \
  -H 'Content-Type: text/plain' --data-binary @- <<'MD'
<conteúdo do agents/<nome>.md>
MD
```

**Importante sobre MCPs:** o plugin **nunca** lê, guarda ou transmite credenciais. Ele só referencia o NOME do MCP server. A configuração (URL, tokens, env vars) vive em `~/.claude/settings.json` e é responsabilidade do Engenheiro. O mini-contexto do motor serve pra dizer ao Claude **quando** acionar o MCP e com **quais restrições**, não **como** autenticar.

## 5c. Se `$ARGUMENTS` = `add firefighter <nome>`

Um **bombeiro** escuta logs (Datadog) e dispara triage quando detecta padrão de erro. Diagnostica e **escala ao Manager** — nunca corrige sozinho. Tem cooldown e teto/hora pra controlar custo.

Conduza:

1. "Qual serviço/área este bombeiro monitora? (ex: api_auth, payments)"
2. "Query Datadog? (ex: `service:api_auth status:error`)"
3. "Threshold — quantos eventos disparam triage? (padrão: 5)"
4. "Janela em minutos pra contar? (padrão: 5)"
5. "Cooldown em segundos entre triages? (padrão: 900 = 15min)"
6. "Teto de triages por hora? (padrão: 4)"
7. "Runbook — primeiros passos quando dispara? (lista de bullets)"
8. "Escalação — quando/como notificar além do Manager? (opcional)"
9. "Emoji? (padrão: 🚒)"

Depois chame:

```bash
curl -sf -X POST "$API/api/agents" \
  -H 'Content-Type: application/json' \
  -d @- <<'JSON'
{ "name": "<nome>", "type": "<specialist|integrator|motor|firefighter>",
  "emoji": "<emoji>", "role": "<uma linha>", "model": "<modelo>",
  "tools": ["Read","Grep","Glob"], "can_talk_to": ["manager"] }
JSON
```

O corpo aceita os mesmos campos do manifesto (seção 8). Depois de criar,
grave o mini-contexto do agente:

```bash
curl -sf -X PUT "$API/api/agents/<nome>/context" \
  -H 'Content-Type: text/plain' --data-binary @- <<'MD'
<conteúdo do agents/<nome>.md>
MD
```

**Rodar o poller:**

```bash
# Loop contínuo (padrão: 60s)
curl -sf "$API/api/data" >/dev/null  # firefighter roda dentro do poupar-http

# Uma iteração só (útil pra testar)
curl -sf "$API/api/data" >/dev/null  # firefighter roda dentro do poupar-http

# Dry-run: consulta Datadog, avalia threshold, NÃO dispara claude
curl -sf "$API/api/data" >/dev/null  # firefighter roda dentro do poupar-http

# Só um bombeiro específico
curl -sf "$API/api/data" >/dev/null  # firefighter roda dentro do poupar-http
```

Precisa das env vars `DD_API_KEY`, `DD_APP_KEY` e opcionalmente `DD_SITE` (padrão `datadoghq.com`). O poller lê do ambiente — **nunca** guarda valores.

Triages viram observações tipo `firefighter_triage` em `observations.jsonl` e aparecem na UI.

## 6. Se `$ARGUMENTS` = `edit <nome>`

Mostre o agente atual + contexto, pergunte o que mudar, edite.

## 7. Se `$ARGUMENTS` = `remove <nome>`

- Se for Manager ou Oracle: avise que são papéis obrigatórios do sistema. A API recusa (400).
- `handoff` é reservado — a API recusa (405).
- Demais: remove do manifest + apaga `agents/<nome>.md`:

```bash
curl -sf -X DELETE "$API/api/agents/<nome>" \
  && echo "removido" || echo "não encontrado (404) ou protegido"
```

## 8. Estrutura do `agents.json`

```json
{
  "version": 1,
  "manager": "manager",
  "oracle":  "oracle",
  "rule":    "Toda decisão relevante deve ser comunicada ao Manager antes de finalizar.",
  "agents": {
    "manager": {
      "emoji": "🎯",
      "model": "opus",
      "role":  "Orquestrador central...",
      "context_file": "agents/manager.md",
      "tools": ["Task", "Read", "Write", "Edit", "Bash"],
      "mcps":  [],
      "can_talk_to": ["oracle", "architect", "reviewer", "tester", "designer"]
    }
  }
}
```

## 9. Como o Claude realmente usa isso

O bloco **"Agentes do projeto"** é injetado no contexto a cada sessão. Quando o Claude ler, vai saber:

1. Qual agente delegar cada tipo de tarefa (via Task tool com `subagent_type`)
2. Que toda decisão volta ao Manager antes de finalizar
3. Que o Oracle é fonte de verdade (mas não dá ordens)
4. Os limites de cada agente (ferramentas, MCPs, quem fala com quem)

As invocações são capturadas pelo hook `PostToolUse` como observações tipo `agent_call` — a UI mostra em tempo quase-real na aba **Agentes**.

## Não confundir com

- **Subagents nativos do Claude Code** — este skill NÃO cria subagents. Só declara a convenção e o Claude usa os subagents que já existem via Task tool.
- **`/mem-soul`** — alma é do projeto, não de agentes individuais.
