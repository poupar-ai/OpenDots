# /mem-agents — Registry de agentes do projeto

Declara os papéis de agentes deste projeto (Manager, Oracle, e especialistas). O plugin injeta essa config no contexto pra que o Claude Code use subagents nativos (via Task tool) de forma consistente — com modelo certo, contexto certo e regras de comunicação claras.

**Entrada:** `$ARGUMENTS` — opcional.
- vazio → lista os agentes declarados
- `init` → cria defaults (Manager, Oracle, Architect, Reviewer, Tester, Designer)
- `init --force` → sobrescreve existentes
- `show <nome>` → mostra mini-contexto de um agente
- `add <nome>` → cria novo agente (pergunta papel, modelo, ferramentas)
- `edit <nome>` → edita um agente existente
- `remove <nome>` → remove um agente (nunca remove Manager ou Oracle sem confirmação)

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

## 5. Se `$ARGUMENTS` = `add <nome>`

Conduza uma conversa curta:

1. "Qual o papel desse agente em uma frase?"
2. "Qual modelo? (opus / sonnet / haiku)"
3. "Quais ferramentas ele usa? (Read, Write, Edit, Bash, Grep, Glob, Task — escolha as relevantes)"
4. "Ele fala com quais outros agentes?"
5. "MCP servers permitidos? (ou nenhum)"
6. "Emoji representativo? (opcional)"

Depois:
- Adiciona entrada no `agents.json`
- Cria `agents/<nome>.md` com template (título + Responsabilidade + Princípios)
- Mostra o que ficou e pergunta se está OK

## 6. Se `$ARGUMENTS` = `edit <nome>`

Mostre o agente atual + contexto, pergunte o que mudar, edite.

## 7. Se `$ARGUMENTS` = `remove <nome>`

- Se for Manager ou Oracle: avise que são papéis obrigatórios do sistema. Pergunte 2x se tem certeza. Se confirmar, remove.
- Demais: remove direto do manifest + apaga `agents/<nome>.md`.

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
