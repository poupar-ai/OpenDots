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

## 1. Encontre o memDir

```bash
ROOT=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
MEM="$ROOT/.claude-project-memory"
```

## 2. Se `$ARGUMENTS` está vazio — listar agentes

```bash
node -e "
  const { loadManifest, countInvocations } = require('${CLAUDE_PLUGIN_ROOT:-/home/tiago/pessoal/project-mem/plugin}/scripts/agents.cjs');
  const cfg = loadManifest('$MEM');
  if (!cfg) { console.log('nenhum agente declarado. Rode /mem-agents init pra criar os defaults.'); process.exit(0); }
  const { counts = {}, recent = {} } = countInvocations('$MEM');
  console.log('Manager:', cfg.manager, '| Oracle:', cfg.oracle);
  console.log('Regra:', cfg.rule || '(nenhuma)');
  console.log();
  for (const [name, a] of Object.entries(cfg.agents)) {
    const n = counts[name] || 0;
    const last = recent[name] ? ' · último uso ' + recent[name].slice(0,10) : '';
    console.log('•', a.emoji || '', name, '—', a.role);
    console.log('    modelo:', a.model, '| invocações:', n + last);
  }
"
```

## 3. Se `$ARGUMENTS` = `init`

Mostra os defaults que serão criados, pergunta confirmação, então chama:

```bash
node -e "
  const { initDefaults } = require('${CLAUDE_PLUGIN_ROOT:-/home/tiago/pessoal/project-mem/plugin}/scripts/agents.cjs');
  const cfg = initDefaults('$MEM', { overwrite: false });
  console.log('Agentes criados:', Object.keys(cfg.agents).join(', '));
"
```

Informe o dev:
- Papéis criados (Manager 🎯, Oracle 🔮, Architect 🏛, Reviewer 🔍, Tester 🧪, Designer 🎨)
- Cada um tem mini-contexto em `.claude-project-memory/agents/<nome>.md` — pode editar pra personalizar pro projeto
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
node -e "
  const a = require('${CLAUDE_PLUGIN_ROOT:-/home/tiago/pessoal/project-mem/plugin}/scripts/agents.cjs');
  a.addAgent('$MEM', '<nome>', {
    type: 'specialist',
    emoji: '<emoji>',
    model: '<modelo>',
    role:  '<papel>',
    tools: [/* lista */],
    can_talk_to: ['manager'/*, ...*/],
    context: '# <nome>\\n\\n## Responsabilidade\\n<papel>\\n\\n## Princípios\\n- ...'
  });
  console.log('ok');
"
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
node -e "
  const a = require('${CLAUDE_PLUGIN_ROOT:-/home/tiago/pessoal/project-mem/plugin}/scripts/agents.cjs');
  const ctx = a.integratorTemplate({
    name:     '<nome>',
    service:  '<serviço>',
    description: 'Conhece a API do <serviço>. Informa sobre endpoints, formatos, erros, webhooks.',
    docs_url: '<url>',
    base_url: '<url>',
    auth:     '<descrição da autenticação>',
    env_vars: [/* nomes */],
    endpoints: [/* { method, path, purpose } */],
    webhooks:  [/* { event, purpose } */],
    gotchas:   []
  });
  a.addAgent('$MEM', '<nome>', {
    type: 'integrator',
    emoji: '🔌',
    model: 'sonnet',
    role:  'Integrator para <serviço>. Informa sobre a API, não age.',
    tools: ['Read', 'Grep', 'Glob'],
    mcps:  [],
    can_talk_to: ['manager'],
    context: ctx
  });
  console.log('ok');
"
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
node -e "
  const a = require('${CLAUDE_PLUGIN_ROOT:-/home/tiago/pessoal/project-mem/plugin}/scripts/agents.cjs');
  const ctx = a.motorTemplate({
    name:   '<nome>',
    action: '<ação>',
    description: 'Motor que executa <ação> no mundo real via MCP.',
    mcps:   [/* nomes */],
    tools:  [/* ferramentas */],
    env_vars: [/* nomes */],
    allowed_actions:   [/* ... */],
    forbidden_actions: [/* ... */],
    requires_manager_approval: true
  });
  a.addAgent('$MEM', '<nome>', {
    type: 'motor',
    emoji: '⚙️',
    model: 'sonnet',
    role:  '<papel curto>',
    tools: [/* ferramentas */],
    mcps:  [/* nomes */],
    can_talk_to: ['manager'],
    context: ctx
  });
  console.log('ok');
"
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
node -e "
  const a = require('${CLAUDE_PLUGIN_ROOT:-/home/tiago/pessoal/project-mem/plugin}/scripts/agents.cjs');
  const ctx = a.firefighterTemplate({
    name: '<nome>',
    service: '<serviço>',
    description: 'Bombeiro para <serviço>. Reativo — escuta logs e dispara triage.',
    datadog_query: '<query>',
    threshold: 5,
    window_minutes: 5,
    cooldown_seconds: 900,
    max_triages_per_hour: 4,
    runbook: [/* bullets */],
    escalation: [/* bullets */]
  });
  a.addAgent('$MEM', '<nome>', {
    type: 'firefighter',
    emoji: '🚒',
    model: 'sonnet',
    role:  'Bombeiro de <serviço>. Escuta Datadog, dispara triage ao cruzar threshold.',
    tools: ['Read', 'Grep', 'Glob'],
    mcps:  [],
    can_talk_to: ['manager'],
    watch: {
      source: 'datadog',
      query: '<query>',
      threshold: 5,
      window_minutes: 5,
      cooldown_seconds: 900,
      max_triages_per_hour: 4,
      env_vars: ['DD_API_KEY', 'DD_APP_KEY', 'DD_SITE']
    },
    context: ctx
  });
  console.log('ok — rode poller com: node ${CLAUDE_PLUGIN_ROOT:-/home/tiago/pessoal/project-mem/plugin}/scripts/firefighter.cjs watch');
"
```

**Rodar o poller:**

```bash
# Loop contínuo (padrão: 60s)
node ${CLAUDE_PLUGIN_ROOT:-/home/tiago/pessoal/project-mem/plugin}/scripts/firefighter.cjs watch

# Uma iteração só (útil pra testar)
node ${CLAUDE_PLUGIN_ROOT:-/home/tiago/pessoal/project-mem/plugin}/scripts/firefighter.cjs watch --once

# Dry-run: consulta Datadog, avalia threshold, NÃO dispara claude
node ${CLAUDE_PLUGIN_ROOT:-/home/tiago/pessoal/project-mem/plugin}/scripts/firefighter.cjs watch --once --dry-run

# Só um bombeiro específico
node ${CLAUDE_PLUGIN_ROOT:-/home/tiago/pessoal/project-mem/plugin}/scripts/firefighter.cjs watch --agent <nome>
```

Precisa das env vars `DD_API_KEY`, `DD_APP_KEY` e opcionalmente `DD_SITE` (padrão `datadoghq.com`). O poller lê do ambiente — **nunca** guarda valores.

Triages viram observações tipo `firefighter_triage` em `observations.jsonl` e aparecem na UI.

## 6. Se `$ARGUMENTS` = `edit <nome>`

Mostre o agente atual + contexto, pergunte o que mudar, edite.

## 7. Se `$ARGUMENTS` = `remove <nome>`

- Se for Manager ou Oracle: avise que são papéis obrigatórios do sistema. Pergunte 2x se tem certeza. Se confirmar, remove.
- Demais: remove direto do manifest + apaga `agents/<nome>.md`:

```bash
node -e "
  const a = require('${CLAUDE_PLUGIN_ROOT:-/home/tiago/pessoal/project-mem/plugin}/scripts/agents.cjs');
  console.log(a.removeAgent('$MEM', '<nome>') ? 'removido' : 'não encontrado');
"
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
