'use strict';

// Registry de agentes por projeto.
// - .claude-project-memory/agents.json — manifest estruturado
// - .claude-project-memory/agents/<nome>.md — mini-contexto de cada agente
//
// Não orquestra. A responsabilidade é declarar papéis + injetar no contexto
// do Claude Code pra que os subagents nativos (Task tool) sejam usados com
// consistência. Toda decisão volta ao Manager (imposto via regra no contexto).

const fs   = require('fs');
const path = require('path');

const MANIFEST = 'agents.json';
const DIR      = 'agents';

// ── Defaults ──────────────────────────────────────────────────────────────────

const DEFAULT_MANIFEST = {
  version: 1,
  manager: 'manager',
  oracle:  'oracle',
  rule: 'Toda decisão relevante deve ser comunicada ao Manager antes de finalizar. O Engenheiro pode falar direto com qualquer agente, mas o Manager é notificado.',
  agents: {
    manager: {
      emoji: '🎯',
      model: 'opus',
      role:  'Orquestrador central. Recebe ordens do Engenheiro, distribui tarefas, consolida resultados, comunica decisões.',
      context_file: 'agents/manager.md',
      tools: ['Task', 'Read', 'Write', 'Edit', 'Bash', 'Grep', 'Glob'],
      mcps:  [],
      can_talk_to: ['oracle', 'architect', 'reviewer', 'tester', 'designer']
    },
    oracle: {
      emoji: '🔮',
      model: 'opus',
      role:  'Conhece todo o projeto — fonte de verdade sobre código, arquitetura e histórico. Responde, não dá ordens.',
      context_file: 'agents/oracle.md',
      tools: ['Read', 'Grep', 'Glob'],
      mcps:  [],
      can_talk_to: ['manager']
    },
    architect: {
      emoji: '🏛',
      model: 'opus',
      role:  'Decisões de arquitetura, trade-offs de design, ADRs.',
      context_file: 'agents/architect.md',
      tools: ['Read', 'Grep', 'Glob', 'Write'],
      mcps:  [],
      can_talk_to: ['manager', 'oracle']
    },
    reviewer: {
      emoji: '🔍',
      model: 'sonnet',
      role:  'Revisa PRs, aponta bugs, sugere melhorias. Segurança e clareza.',
      context_file: 'agents/reviewer.md',
      tools: ['Read', 'Grep', 'Glob'],
      mcps:  [],
      can_talk_to: ['manager', 'oracle']
    },
    tester: {
      emoji: '🧪',
      model: 'sonnet',
      role:  'Escreve testes, roda suites, reporta regressões. Unit + integration.',
      context_file: 'agents/tester.md',
      tools: ['Read', 'Write', 'Edit', 'Bash', 'Grep'],
      mcps:  [],
      can_talk_to: ['manager', 'oracle']
    },
    designer: {
      emoji: '🎨',
      model: 'sonnet',
      role:  'UI/UX, layout, acessibilidade, consistência visual.',
      context_file: 'agents/designer.md',
      tools: ['Read', 'Write', 'Edit', 'Grep', 'Glob'],
      mcps:  [],
      can_talk_to: ['manager']
    }
  }
};

const DEFAULT_MINI_CONTEXTS = {
  manager: `# Manager — Orquestrador

## Responsabilidade
Você é o Manager deste projeto. Recebe ordens do Engenheiro humano, distribui tarefas para os agentes especializados, consolida resultados, e comunica decisões de volta ao Engenheiro.

## Princípios
- **Uma decisão, um dono.** Se duas áreas entram em conflito, o Manager resolve (ou escala ao Engenheiro).
- **Transparência.** Todas as trocas relevantes entre agentes passam por você.
- **Economia de contexto.** Só chame um agente quando a especialidade dele for necessária. Não crie ruído.

## Fluxo típico
1. Receber pedido do Engenheiro
2. Decompor em sub-tarefas
3. Delegar via Task tool (escolhendo o agente certo)
4. Consolidar respostas
5. Reportar ao Engenheiro com 1 decisão clara
`,
  oracle: `# Oracle — Fonte de verdade

## Responsabilidade
Você conhece o projeto inteiro — código, arquitetura, decisões passadas, histórico. Responde perguntas, **nunca dá ordens**.

## Princípios
- **Cite fontes.** Sempre referencie arquivo:linha quando falar sobre código.
- **Não suponha.** Se não sabe, diga. Não invente arquitetura.
- **Direto.** Respostas curtas, sem floreio.

## Quando é chamado
- "Onde está implementada a função X?"
- "Como o auth funciona aqui?"
- "Esse padrão já existe no projeto?"
- Consultas antes de decisões do Architect/Reviewer
`,
  architect: `# Architect — Decisões de design

## Responsabilidade
Decisões de arquitetura, trade-offs, escolha de padrões. Produz ADRs quando a decisão é relevante.

## Princípios
- **Simplicidade primeiro.** Complexidade só se justificada.
- **Cita alternativas.** Sempre mostra 2-3 opções antes de recomendar.
- **Horizonte claro.** Diz qual decisão é reversível, qual não é.
`,
  reviewer: `# Reviewer — Revisão de código

## Responsabilidade
Revisa PRs, aponta bugs, sugere melhorias. Foco em segurança, clareza e manutenibilidade.

## Princípios
- **Específico.** Aponta arquivo:linha e explica o porquê.
- **Proporcional.** Distingue nit de bloqueante.
- **Construtivo.** Sugere como arrumar, não só o que está errado.
`,
  tester: `# Tester — Qualidade

## Responsabilidade
Escreve testes (unit + integration), roda suites, reporta regressões. Garante que mudanças não quebram o existente.

## Princípios
- **Cobre o caminho feliz e 1-2 edges.** Não testa tudo.
- **Testes rápidos.** Se um teste é lento demais, sinaliza.
- **Fail informativo.** Mensagens de erro que explicam o que quebrou.
`,
  designer: `# Designer — UI/UX

## Responsabilidade
Decisões visuais, layout, acessibilidade, consistência. Conversa com o Manager antes de mudanças grandes.

## Princípios
- **Acessível por padrão.** Contraste, aria, foco.
- **Consistente com a alma.** Respeita a voz e estética do SOUL.md.
- **Mobile-first quando relevante.**
`
};

// ── I/O ───────────────────────────────────────────────────────────────────────

function manifestPath(memDir) { return path.join(memDir, MANIFEST); }
function agentsDir(memDir)    { return path.join(memDir, DIR); }

function loadManifest(memDir) {
  const p = manifestPath(memDir);
  if (!fs.existsSync(p)) return null;
  try { return JSON.parse(fs.readFileSync(p, 'utf8')); }
  catch { return null; }
}

function saveManifest(memDir, cfg) {
  fs.writeFileSync(manifestPath(memDir), JSON.stringify(cfg, null, 2) + '\n');
}

function loadMiniContext(memDir, contextFile) {
  const p = path.join(memDir, contextFile);
  if (!fs.existsSync(p)) return null;
  try { return fs.readFileSync(p, 'utf8').trim(); }
  catch { return null; }
}

// ── Init com defaults ─────────────────────────────────────────────────────────

function initDefaults(memDir, { overwrite = false } = {}) {
  const dir = agentsDir(memDir);
  if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });

  if (overwrite || !fs.existsSync(manifestPath(memDir))) {
    saveManifest(memDir, DEFAULT_MANIFEST);
  }

  for (const [name, content] of Object.entries(DEFAULT_MINI_CONTEXTS)) {
    const p = path.join(dir, `${name}.md`);
    if (overwrite || !fs.existsSync(p)) fs.writeFileSync(p, content);
  }
  return loadManifest(memDir);
}

// ── Stats de invocação (contagem via observations.jsonl) ──────────────────────

function countInvocations(memDir) {
  const obsFile = path.join(memDir, 'observations.jsonl');
  if (!fs.existsSync(obsFile)) return {};

  const counts = {};
  const recent = {};
  try {
    const lines = fs.readFileSync(obsFile, 'utf8').split('\n').filter(Boolean);
    for (const l of lines) {
      let o; try { o = JSON.parse(l); } catch { continue; }
      if (o.type !== 'agent_call') continue;
      const name = (o.agent || '').toLowerCase();
      if (!name) continue;
      counts[name] = (counts[name] || 0) + 1;
      if (!recent[name] || (o.ts || '') > recent[name]) recent[name] = o.ts;
    }
  } catch {}

  return { counts, recent };
}

// ── Bloco injetável pro session-init ──────────────────────────────────────────

function buildInjectionBlock(memDir) {
  const cfg = loadManifest(memDir);
  if (!cfg || !cfg.agents) return [];

  const lines = ['## Agentes do projeto'];

  if (cfg.rule) {
    lines.push(`> **Regra de ouro:** ${cfg.rule}`);
    lines.push('');
  }

  const manager = cfg.manager || 'manager';
  const entries = Object.entries(cfg.agents);

  lines.push(`Papéis declarados (delegue via Task tool com \`subagent_type\` apropriado):`);
  for (const [name, a] of entries) {
    const emoji = a.emoji || '•';
    const isManager = name === manager;
    const tag = isManager ? ' **(Manager)**' : (name === cfg.oracle ? ' _(Oracle)_' : '');
    lines.push(`- ${emoji} **${name}**${tag} — ${a.role}`);
    if (a.model) lines.push(`  · modelo: \`${a.model}\``);
    if (a.can_talk_to && a.can_talk_to.length) {
      lines.push(`  · fala com: ${a.can_talk_to.map(x => `\`${x}\``).join(', ')}`);
    }
    const ctx = loadMiniContext(memDir, a.context_file);
    if (ctx) {
      const short = ctx.split('\n').slice(0, 4).join(' ').replace(/#+\s*/g, '').slice(0, 140);
      if (short) lines.push(`  · _${short}…_`);
    }
  }
  lines.push('');
  lines.push(`> Antes de finalizar uma tarefa complexa: delegue pro agente certo (Task) e reporte resultado consolidado ao Manager.`);

  return lines;
}

module.exports = {
  loadManifest,
  saveManifest,
  loadMiniContext,
  initDefaults,
  countInvocations,
  buildInjectionBlock,
  manifestPath,
  agentsDir,
  DEFAULT_MANIFEST,
  DEFAULT_MINI_CONTEXTS
};
