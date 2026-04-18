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

// Tipos de agente:
//   core        — Manager, Oracle (sistema, sempre existem)
//   specialist  — Conhecimento interno do projeto (architect, reviewer, tester, designer)
//   integrator  — Conhece uma API externa a fundo. INFORMA, NÃO AGE. Sem MCPs.
//   motor       — Age no mundo real via MCPs. Guardrails obrigatórios.
//   firefighter — Reativo. Escuta logs (Datadog), detecta erro, dispara triage via claude -p.
//                 Diagnostica e escala ao Manager — não corrige sozinho.
const AGENT_TYPES = new Set(['core', 'specialist', 'integrator', 'motor', 'firefighter']);

// Níveis de autonomia:
//   low    — escala quase tudo. Só responde factual sozinho.
//   medium — decide em tarefa clara/reversível; escala ambíguo/consequência.
//   high   — decide e executa; só escala pelos casos listados em call_human_when.
const AUTONOMY_LEVELS = new Set(['low', 'medium', 'high']);

const DEFAULT_MANIFEST = {
  version: 1,
  manager: 'manager',
  oracle:  'oracle',
  rule: 'Toda decisão relevante deve ser comunicada ao Manager antes de finalizar. O Engenheiro pode falar direto com qualquer agente, mas o Manager é notificado. Motores (ação no mundo real) sempre pedem confirmação ao Manager antes de executar.',
  agents: {
    manager: {
      type: 'core',
      emoji: '🎯',
      model: 'opus-4.7',
      role:  'Orquestrador central. Recebe ordens do Engenheiro, distribui tarefas, consolida resultados, comunica decisões.',
      context_file: 'agents/manager.md',
      tools: ['Task', 'Read', 'Write', 'Edit', 'Bash', 'Grep', 'Glob'],
      mcps:  [],
      can_talk_to: ['oracle', 'architect', 'reviewer', 'tester', 'designer', 'pipeline_writer'],
      autonomy: {
        level: 'high',
        call_human_when: [
          'decisão envolve gastar dinheiro da empresa',
          'mudança vai pra produção',
          'decisão muda roadmap ou estratégia',
          'dois especialistas discordam e o manager não tem dados para decidir'
        ]
      }
    },
    oracle: {
      type: 'core',
      emoji: '🔮',
      model: 'opus-4.6',
      role:  'Conhece todo o projeto — fonte de verdade sobre código, arquitetura e histórico. Responde, não dá ordens.',
      context_file: 'agents/oracle.md',
      tools: ['Read', 'Grep', 'Glob'],
      mcps:  [],
      can_talk_to: ['manager'],
      autonomy: {
        level: 'high',
        call_human_when: [
          'pergunta exige acesso a sistema externo que o oracle não tem'
        ]
      }
    },
    architect: {
      type: 'specialist',
      emoji: '🏛',
      model: 'opus-4.6',
      role:  'Decisões de arquitetura, trade-offs de design, ADRs.',
      context_file: 'agents/architect.md',
      tools: ['Read', 'Grep', 'Glob', 'Write'],
      mcps:  [],
      can_talk_to: ['manager', 'oracle'],
      autonomy: {
        level: 'medium',
        call_human_when: [
          'decisão arquitetural é irreversível',
          'trade-off envolve custo recorrente',
          'mudança de stack ou de linguagem'
        ]
      }
    },
    reviewer: {
      type: 'specialist',
      emoji: '🔍',
      model: 'sonnet-4.6',
      role:  'Revisa PRs, aponta bugs, sugere melhorias. Segurança e clareza.',
      context_file: 'agents/reviewer.md',
      tools: ['Read', 'Grep', 'Glob'],
      mcps:  [],
      can_talk_to: ['manager', 'oracle'],
      autonomy: {
        level: 'high',
        call_human_when: [
          'falha crítica de segurança que precisa decisão de bloqueio de merge',
          'PR foge do escopo declarado de forma que afeta outras áreas'
        ]
      }
    },
    tester: {
      type: 'specialist',
      emoji: '🧪',
      model: 'sonnet-4.6',
      role:  'Escreve testes, roda suites, reporta regressões. Unit + integration.',
      context_file: 'agents/tester.md',
      tools: ['Read', 'Write', 'Edit', 'Bash', 'Grep'],
      mcps:  [],
      can_talk_to: ['manager', 'oracle'],
      autonomy: {
        level: 'high',
        call_human_when: [
          'teste revela bug em código já em produção'
        ]
      }
    },
    designer: {
      type: 'specialist',
      emoji: '🎨',
      model: 'sonnet-4.6',
      role:  'UI/UX, layout, acessibilidade, consistência visual.',
      context_file: 'agents/designer.md',
      tools: ['Read', 'Write', 'Edit', 'Grep', 'Glob'],
      mcps:  [],
      can_talk_to: ['manager'],
      autonomy: {
        level: 'medium',
        call_human_when: [
          'mudança visual afeta identidade de marca',
          'decisão de UX que muda fluxo principal do produto'
        ]
      }
    },
    pipeline_writer: {
      type: 'core',
      emoji: '✨',
      model: 'opus-4.6',
      role:  'Transforma descrições humanas em pipelines YAML reproduzíveis. Conversa com os agentes envolvidos pra entender o fluxo, depois grava pipeline.yaml pronto pra rodar.',
      context_file: 'agents/pipeline_writer.md',
      tools: ['Read', 'Write', 'Edit', 'Grep', 'Glob', 'Bash'],
      mcps:  [],
      can_talk_to: ['manager', 'oracle', 'architect', 'reviewer', 'tester', 'designer'],
      autonomy: {
        level: 'medium',
        call_human_when: [
          'não está claro qual sequência de agentes o humano quer',
          'mais de um caminho razoável existe e escolher errado cria retrabalho',
          'pipeline precisaria de agente que não existe no time'
        ]
      }
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
`,
  pipeline_writer: `# Pipeline Writer — Gravador de Fluxos

## Responsabilidade
Transformar descrição humana ("quero uma pipeline pra X") em **pipeline YAML válida** que roda no runner.
Conversa com os agentes envolvidos pra entender o fluxo real, decidir a ordem dos passos e onde entram checkpoints humanos.

## Princípios
- **Fluxo real, não ideal.** Só steps que o humano pediu.
- **Checkpoints onde dói errar.** Antes de gastar tempo caro, antes de efeito externo (publicar, cobrar, enviar).
- **IDs são slugs.** \`[a-z][a-z0-9-]+[a-z0-9]\`. Nunca espaço, acento, maiúscula.
- **Cada step tem dono.** \`agent: <nome>\` pra steps executáveis; \`type: checkpoint\` + \`owner\` pra gates humanos.
- **Pergunte antes de inventar.** Se não ficou claro quem faz X, consulte o humano ou o agente em dúvida.

## Onde grava
Sempre no projeto corrente: \`.claude-project-memory/pipelines/<id>/pipeline.yaml\`.
Template global é trabalho do humano (não tem acesso a \`plugin/pipelines/\`).

## Schema mínimo
\`\`\`yaml
id: <slug>
label: <string>
description: |
  multi-linha opcional
tags: [a, b]
steps:
  - id: <slug>
    agent: <nome-agente>
    label: <string>
    description: <string>
    best_practices: [x, y]   # opcional
  - id: <slug>
    type: checkpoint
    label: <string>
    owner: any_human
\`\`\`

## Fluxo típico
1. Entenda o pedido. Pergunte se faltar dado.
2. Consulte agentes envolvidos via handoff se precisar (ex: "designer, você faz só copy ou copy+arte?"). Só se a resposta muda o YAML.
3. Esboce 3-10 steps.
4. Coloque checkpoints antes de ação cara/irreversível. Nunca como 1º step.
5. Valide: IDs slug válidos e únicos, agentes existem, checkpoints têm owner, fluxo ponta a ponta faz sentido.
6. Write no arquivo e confirma ao humano: id, path, 1 linha por step.
`
};

// ── Templates pra novos agentes ───────────────────────────────────────────────

function integratorTemplate({ name, service, description, base_url, auth, env_vars, endpoints, webhooks, docs_url, gotchas }) {
  const envList = (env_vars || []).map(v => `- \`${v}\``).join('\n') || '- _(nenhuma)_';
  const epList  = (endpoints || []).map(e => `- \`${e.method || 'GET'} ${e.path}\`${e.purpose ? ' — ' + e.purpose : ''}`).join('\n') || '- _A preencher a partir da documentação oficial._';
  const whList  = (webhooks || []).map(w => `- \`${w.event}\`${w.purpose ? ' — ' + w.purpose : ''}`).join('\n') || '- _(sem webhooks documentados ainda)_';
  const gotList = (gotchas || []).map(g => `- ${g}`).join('\n') || '- _A preencher conforme integração avança._';

  return `# ${name} — Integrator (${service || name})

## Papel
${description || 'Integrator agent. Conhece a API externa a fundo. **Informa, não age.** Quem age é o motor correspondente.'}

## Documentação oficial
${docs_url ? `- ${docs_url}` : '- _Adicionar URL da documentação oficial_'}

## Base URL
${base_url ? `\`${base_url}\`` : '_A definir_'}

## Autenticação
${auth || '_A preencher: tipo (API key / OAuth / JWT), header esperado, formato do token._'}

## Env vars esperadas
${envList}

> Nunca guarde valores no código ou nos contextos. Só nomes.

## Endpoints principais
${epList}

## Webhooks
${whList}

## Gotchas conhecidas
${gotList}

## Quando o Manager deve consultar este agente
- Antes de implementar qualquer chamada pra ${service || name}
- Quando aparecer erro de integração
- Ao revisar PRs que mexem com ${service || name}
`;
}

function firefighterTemplate({ name, service, description, datadog_query, threshold, window_minutes, cooldown_seconds, max_triages_per_hour, runbook, escalation, env_vars }) {
  const envList = (env_vars || ['DD_API_KEY', 'DD_APP_KEY', 'DD_SITE']).map(v => `- \`${v}\``).join('\n');
  const runbookList = (runbook || []).map(r => `- ${r}`).join('\n') || '- _A preencher: primeiro olhar, comandos úteis, hipóteses típicas._';
  const escalationList = (escalation || []).map(e => `- ${e}`).join('\n') || '- Sempre: reportar triage ao Manager\n- Se incidente crítico: notificar o Engenheiro humano';

  return `# ${name} — Firefighter (${service || 'monitoramento'})

## Papel
${description || 'Firefighter agent. **Reativo** — escuta logs e dispara triage quando detecta padrão de erro. Diagnostica, propõe correção, **escala ao Manager**. Nunca corrige sozinho.'}

## O que observa
- **Fonte:** Datadog Logs
- **Query:** \`${datadog_query || 'service:? status:error'}\`
- **Threshold:** ${threshold || 5} eventos
- **Janela:** últimos ${window_minutes || 5} minutos

## Controles de custo
- **Cooldown:** ${cooldown_seconds || 900}s entre triages consecutivas (evita spam no mesmo incidente)
- **Teto:** ${max_triages_per_hour || 4} triages/hora (proteção contra storm)

## Env vars esperadas (só NOMES, sem valores)
${envList}

## Runbook — primeiro olhar
${runbookList}

## Escalação
${escalationList}

## Regras obrigatórias
1. **Nunca** corrige código sem aprovação do Manager + Engenheiro.
2. **Sempre** registra a triage como observação (\`firefighter_triage\`).
3. **Não** re-dispara durante cooldown (dedup por incidente).
4. **Dry-first**: diagnóstico primeiro, ação depois — e só via motor aprovado.

## Quando o Manager consulta
- Após receber triage: validar diagnóstico, decidir se aciona motor de correção ou só monitora.
- Para ajustar threshold/query se estiver ruidoso.
`;
}

function motorTemplate({ name, action, description, mcps, tools, env_vars, allowed_actions, forbidden_actions, requires_manager_approval }) {
  const mcpList = (mcps || []).map(m => `- \`${m}\``).join('\n') || '- _(nenhum MCP configurado — este motor precisa de um MCP server em settings.json para agir)_';
  const envList = (env_vars || []).map(v => `- \`${v}\``).join('\n') || '- _(nenhuma)_';
  const allowed = (allowed_actions || []).map(a => `- ${a}`).join('\n') || '- _A definir_';
  const forbidden = (forbidden_actions || []).map(a => `- ${a}`).join('\n') || '- Enviar conteúdo não verificado\n- Operar fora do horário comercial (quando aplicável)\n- Ações em massa sem aprovação do Manager';

  return `# ${name} — Motor (${action || name})

## Papel
${description || 'Motor agent. **Age no mundo real** via MCPs ou chamadas de rede. Toda ação relevante passa pelo Manager.'}

## Guardrails obrigatórios
1. **Confirmar com o Manager** antes de executar qualquer ação ${requires_manager_approval === false ? '_(exceto operações marcadas como seguras)_' : 'relevante'}
2. **Nunca** armazenar ou logar conteúdo sensível (senhas, tokens, dados pessoais em texto plano)
3. **Idempotência** — toda ação precisa ter identificador único pra evitar duplicidade
4. **Dry-run primeiro** quando possível — simule antes de executar de verdade

## MCPs necessários
${mcpList}

## Ferramentas permitidas
${(tools || []).map(t => `- \`${t}\``).join('\n') || '- _A definir_'}

## Env vars
${envList}

## Ações permitidas
${allowed}

## Ações proibidas
${forbidden}

## Quando o Manager pode invocar
- Apenas após confirmar com o Engenheiro humano em ações de escopo amplo
- Individualmente pra ações rotineiras já aprovadas
`;
}

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

function saveMiniContext(memDir, contextFile, content) {
  if (typeof content !== 'string') throw new Error('content precisa ser string');
  if (content.length > 200_000) throw new Error('mini-contexto muito longo (máx 200KB)');
  const rel = contextFile || '';
  if (!rel || rel.includes('..') || path.isAbsolute(rel)) {
    throw new Error('context_file inválido');
  }
  const p = path.join(memDir, rel);
  const dir = path.dirname(p);
  if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
  const normalized = content.endsWith('\n') ? content : content + '\n';
  fs.writeFileSync(p, normalized);
  return p;
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

  const groups = {
    core:        { title: '### Core (sistema)',                                           list: [] },
    specialist:  { title: '### Especialistas (conhecimento interno)',                     list: [] },
    integrator:  { title: '### Integradores (conhecimento externo — consultam, não agem)',list: [] },
    motor:       { title: '### Motores (ação no mundo — requer confirmação do Manager)',  list: [] },
    firefighter: { title: '### Bombeiros (reativos — escutam logs, diagnosticam, escalam)',list: [] }
  };

  for (const [name, a] of entries) {
    const type = AGENT_TYPES.has(a.type) ? a.type : 'specialist';
    groups[type].list.push([name, a]);
  }

  lines.push(`Delegue via Task tool com \`subagent_type\` apropriado. Respeite o tipo de cada agente:`);

  for (const key of ['core', 'specialist', 'integrator', 'motor', 'firefighter']) {
    const g = groups[key];
    if (g.list.length === 0) continue;
    lines.push('');
    lines.push(g.title);
    for (const [name, a] of g.list) {
      const emoji = a.emoji || '•';
      const isManager = name === manager;
      const tag = isManager ? ' **(Manager)**' : (name === cfg.oracle ? ' _(Oracle)_' : '');
      lines.push(`- ${emoji} **${name}**${tag} — ${a.role}`);
      if (a.model) lines.push(`  · modelo: \`${a.model}\``);
      if (a.mcps && a.mcps.length) {
        lines.push(`  · MCPs: ${a.mcps.map(x => `\`${x}\``).join(', ')}`);
      }
      if (a.watch && a.watch.query) {
        lines.push(`  · observa: \`${a.watch.query}\` (${a.watch.source || 'datadog'}, threshold ${a.watch.threshold}/${a.watch.window_minutes}min)`);
      }
      if (a.can_talk_to && a.can_talk_to.length) {
        lines.push(`  · fala com: ${a.can_talk_to.map(x => `\`${x}\``).join(', ')}`);
      }
      const ctx = loadMiniContext(memDir, a.context_file);
      if (ctx) {
        const short = ctx.split('\n').slice(0, 4).join(' ').replace(/#+\s*/g, '').slice(0, 140);
        if (short) lines.push(`  · _${short}…_`);
      }
    }
  }

  lines.push('');
  lines.push(`> Antes de finalizar uma tarefa complexa: delegue pro agente certo (Task) e reporte resultado consolidado ao Manager. Motores nunca executam sem confirmação.`);

  return lines;
}

// ── Adicionar / remover agentes ───────────────────────────────────────────────

function addAgent(memDir, name, config) {
  if (!name || !/^[a-z0-9_-]+$/.test(name)) {
    throw new Error('Nome inválido. Use só letras minúsculas, números, "-" ou "_".');
  }
  const type = config.type;
  if (!AGENT_TYPES.has(type)) {
    throw new Error(`Tipo inválido: ${type}. Use um de: ${[...AGENT_TYPES].join(', ')}`);
  }

  const cfg = loadManifest(memDir) || { version: 1, manager: 'manager', oracle: 'oracle', agents: {} };
  if (cfg.agents[name]) {
    throw new Error(`Agente "${name}" já existe. Use edit ou remove primeiro.`);
  }

  const contextFile = config.context_file || `agents/${name}.md`;
  const defaultEmoji = { integrator: '🔌', motor: '⚙️', firefighter: '🚒' }[type] || '•';
  const genderClean = ['male', 'female', 'other'].includes(config.gender) ? config.gender : undefined;
  cfg.agents[name] = {
    type,
    emoji: config.emoji || defaultEmoji,
    ...(genderClean ? { gender: genderClean } : {}),
    model: config.model || 'sonnet-4.6',
    role:  config.role || '',
    context_file: contextFile,
    tools: config.tools || ['Read', 'Grep', 'Glob'],
    mcps:  config.mcps  || [],
    can_talk_to: config.can_talk_to || ['manager'],
    scope: sanitizeScope(config.scope),
  };
  if (type === 'firefighter' && config.watch) {
    cfg.agents[name].watch = {
      source:               config.watch.source               || 'datadog',
      query:                config.watch.query                || '',
      threshold:            config.watch.threshold            || 5,
      window_minutes:       config.watch.window_minutes       || 5,
      cooldown_seconds:     config.watch.cooldown_seconds     || 900,
      max_triages_per_hour: config.watch.max_triages_per_hour || 4,
      env_vars:             config.watch.env_vars             || ['DD_API_KEY', 'DD_APP_KEY', 'DD_SITE']
    };
  }

  saveManifest(memDir, cfg);

  const ctxPath = path.join(memDir, contextFile);
  if (!fs.existsSync(ctxPath)) {
    const dir = path.dirname(ctxPath);
    if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(ctxPath, config.context || `# ${name}\n\n## Responsabilidade\n${config.role || '_A preencher_'}\n`);
  }

  return cfg.agents[name];
}

// ── Autonomia ─────────────────────────────────────────────────────────────────

function normalizeAutonomy(input) {
  if (input === null) return null; // permite limpar
  if (!input || typeof input !== 'object') {
    throw new Error('autonomy precisa ser objeto { level, call_human_when } ou null');
  }
  const level = input.level;
  if (!AUTONOMY_LEVELS.has(level)) {
    throw new Error(`autonomy.level inválido. Use: ${[...AUTONOMY_LEVELS].join(', ')}`);
  }
  const raw = Array.isArray(input.call_human_when) ? input.call_human_when : [];
  const call_human_when = raw
    .map(s => typeof s === 'string' ? s.trim() : '')
    .filter(Boolean);
  return { level, call_human_when };
}

function updateAutonomy(memDir, name, autonomy) {
  const cfg = loadManifest(memDir);
  if (!cfg || !cfg.agents || !cfg.agents[name]) {
    throw new Error(`agente "${name}" não existe em ${manifestPath(memDir)}`);
  }
  const normalized = normalizeAutonomy(autonomy);
  if (normalized === null) {
    delete cfg.agents[name].autonomy;
  } else {
    cfg.agents[name].autonomy = normalized;
  }
  saveManifest(memDir, cfg);
  return cfg.agents[name];
}

// Propaga autonomia ao arquivo de seed (template). `seedAgentsJsonPath` é o
// caminho absoluto pro `plugin/seeds/<cat>/<slug>/agents.json`. Retorna o
// agents.json atualizado, ou null se o agente não existir no seed.
function propagateAutonomyToSeed(seedAgentsJsonPath, name, autonomy) {
  if (!fs.existsSync(seedAgentsJsonPath)) {
    throw new Error(`seed agents.json não encontrado: ${seedAgentsJsonPath}`);
  }
  const seed = JSON.parse(fs.readFileSync(seedAgentsJsonPath, 'utf8'));
  if (!seed.agents || !seed.agents[name]) return null;
  const normalized = normalizeAutonomy(autonomy);
  if (normalized === null) {
    delete seed.agents[name].autonomy;
  } else {
    seed.agents[name].autonomy = normalized;
  }
  fs.writeFileSync(seedAgentsJsonPath, JSON.stringify(seed, null, 2) + '\n');
  return seed.agents[name];
}

// ── Modelo ────────────────────────────────────────────────────────────────────

function normalizeModel(input) {
  if (input === null || input === '') return null; // limpa (usa default do CLI)
  if (typeof input !== 'string') {
    throw new Error('model precisa ser string (ex: "opus", "opus-4.7", "claude-sonnet-4-6") ou null');
  }
  const v = input.trim();
  if (!v) return null;
  if (v.length > 80) throw new Error('model muito longo (máx 80 chars)');
  return v;
}

// Converte o modelo armazenado (aliases versionados tipo "opus-4.7") no que o
// `claude --model` aceita: alias puro (opus/sonnet/haiku) ou ID completo
// (claude-opus-4-7). Passthrough para strings que já começam com "claude-".
const CLI_MODEL_MAP = {
  'opus-4.7': 'claude-opus-4-7',
  'opus-4.6': 'claude-opus-4-6',
  'opus-4.5': 'claude-opus-4-5',
  'opus-4.1': 'claude-opus-4-1',
  'opus-4': 'claude-opus-4-0',
  'sonnet-4.6': 'claude-sonnet-4-6',
  'sonnet-4.5': 'claude-sonnet-4-5',
  'sonnet-4': 'claude-sonnet-4-0',
  'sonnet-3.7': 'claude-3-7-sonnet-latest',
  'haiku-4.5': 'claude-haiku-4-5-20251001',
  'haiku-3.5': 'claude-3-5-haiku-latest',
};

function resolveCliModel(model) {
  if (!model || typeof model !== 'string') return null;
  const v = model.trim();
  if (!v) return null;
  if (v === 'opus' || v === 'sonnet' || v === 'haiku') return v;
  if (v.startsWith('claude-')) return v;
  if (CLI_MODEL_MAP[v]) return CLI_MODEL_MAP[v];
  // Fallback: "X-Y.Z" → "claude-X-Y-Z"
  const m = v.match(/^(opus|sonnet|haiku)-(\d+)\.(\d+)$/);
  if (m) return `claude-${m[1]}-${m[2]}-${m[3]}`;
  return v; // deixa o CLI rejeitar e surfar o erro real
}

function updateModel(memDir, name, model) {
  const cfg = loadManifest(memDir);
  if (!cfg || !cfg.agents || !cfg.agents[name]) {
    throw new Error(`agente "${name}" não existe em ${manifestPath(memDir)}`);
  }
  const normalized = normalizeModel(model);
  if (normalized === null) {
    delete cfg.agents[name].model;
  } else {
    cfg.agents[name].model = normalized;
  }
  saveManifest(memDir, cfg);
  return cfg.agents[name];
}

function propagateModelToSeed(seedAgentsJsonPath, name, model) {
  if (!fs.existsSync(seedAgentsJsonPath)) {
    throw new Error(`seed agents.json não encontrado: ${seedAgentsJsonPath}`);
  }
  const seed = JSON.parse(fs.readFileSync(seedAgentsJsonPath, 'utf8'));
  if (!seed.agents || !seed.agents[name]) return null;
  const normalized = normalizeModel(model);
  if (normalized === null) {
    delete seed.agents[name].model;
  } else {
    seed.agents[name].model = normalized;
  }
  fs.writeFileSync(seedAgentsJsonPath, JSON.stringify(seed, null, 2) + '\n');
  return seed.agents[name];
}

// ── Escopo (sandbox) ─────────────────────────────────────────────────────────
//
// O campo `scope` complementa `tools` com controles finos:
//   - bash_allow: padrões permitidos pro Bash (ex: "git *", "npm run *")
//                 Sem esta lista, Bash é efetivamente DESATIVADO no runtime —
//                 mesmo que 'Bash' esteja em `tools`.
//
// Denylist de Bash (rm, sudo, etc) é GLOBAL no server — não vive aqui.

const KNOWN_TOOL_NAMES = new Set([
  'Read', 'Write', 'Edit', 'Bash', 'Grep', 'Glob',
  'WebFetch', 'WebSearch', 'Task', 'TodoWrite', 'NotebookEdit',
]);

function sanitizeTools(input) {
  if (!Array.isArray(input)) return undefined;
  const out = [];
  const seen = new Set();
  for (const t of input) {
    if (typeof t !== 'string') continue;
    const k = t.trim();
    if (!k || !KNOWN_TOOL_NAMES.has(k) || seen.has(k)) continue;
    seen.add(k);
    out.push(k);
  }
  return out;
}

function sanitizeScope(input) {
  if (!input || typeof input !== 'object') return { bash_allow: [] };
  const bashAllow = Array.isArray(input.bash_allow)
    ? input.bash_allow
        .filter(p => typeof p === 'string' && p.trim().length && p.length <= 200)
        .map(p => p.trim())
        .slice(0, 50)
    : [];
  return { bash_allow: bashAllow };
}

function updateScope(memDir, name, { tools, scope } = {}) {
  const cfg = loadManifest(memDir);
  if (!cfg || !cfg.agents || !cfg.agents[name]) {
    throw new Error(`agente "${name}" não existe em ${manifestPath(memDir)}`);
  }
  const cleanTools = sanitizeTools(tools);
  if (cleanTools !== undefined) cfg.agents[name].tools = cleanTools;
  if (scope !== undefined) cfg.agents[name].scope = sanitizeScope(scope);
  saveManifest(memDir, cfg);
  return cfg.agents[name];
}

// Sanitização dos nomes de secret que o agente pode ler. Mesmo regex do vault.setSecret
// (UPPER_SNAKE) pra garantir consistência — UI envia a lista que o humano marcou,
// mas quem decide o que é nome válido é aqui.
const SECRET_NAME_RE = /^[A-Z][A-Z0-9_]{0,63}$/;

function sanitizeSecretNames(input) {
  if (!Array.isArray(input)) return [];
  const out = [];
  const seen = new Set();
  for (const n of input) {
    if (typeof n !== 'string') continue;
    const k = n.trim();
    if (!k || !SECRET_NAME_RE.test(k) || seen.has(k)) continue;
    seen.add(k);
    out.push(k);
    if (out.length >= 50) break;
  }
  return out.sort();
}

// Define a lista de secrets que o agente recebe como env vars no spawn.
// Só os nomes listados aqui saem do cofre via `vault.getSecrets` — é o ACL.
function updateVaultAccess(memDir, name, { secrets } = {}) {
  const cfg = loadManifest(memDir);
  if (!cfg || !cfg.agents || !cfg.agents[name]) {
    throw new Error(`agente "${name}" não existe em ${manifestPath(memDir)}`);
  }
  const cleanSecrets = sanitizeSecretNames(secrets);
  if (cleanSecrets.length) cfg.agents[name].secrets = cleanSecrets;
  else delete cfg.agents[name].secrets;
  saveManifest(memDir, cfg);
  return cfg.agents[name];
}

function removeAgent(memDir, name) {
  const cfg = loadManifest(memDir);
  if (!cfg || !cfg.agents[name]) return false;
  const ctxFile = cfg.agents[name].context_file;
  delete cfg.agents[name];
  saveManifest(memDir, cfg);
  try {
    const p = path.join(memDir, ctxFile || `agents/${name}.md`);
    if (fs.existsSync(p)) fs.unlinkSync(p);
  } catch {}
  return true;
}

module.exports = {
  loadManifest,
  saveManifest,
  loadMiniContext,
  saveMiniContext,
  initDefaults,
  countInvocations,
  buildInjectionBlock,
  addAgent,
  removeAgent,
  updateAutonomy,
  propagateAutonomyToSeed,
  normalizeAutonomy,
  updateScope,
  sanitizeScope,
  sanitizeTools,
  updateVaultAccess,
  sanitizeSecretNames,
  KNOWN_TOOL_NAMES,
  updateModel,
  propagateModelToSeed,
  normalizeModel,
  resolveCliModel,
  integratorTemplate,
  motorTemplate,
  firefighterTemplate,
  manifestPath,
  agentsDir,
  AGENT_TYPES,
  AUTONOMY_LEVELS,
  DEFAULT_MANIFEST,
  DEFAULT_MINI_CONTEXTS
};
