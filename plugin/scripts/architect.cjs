'use strict';

// Arquiteto — agente especial que conversa com o humano e gera seeds custom.
//
// DNA original project-mem: o arquiteto é um AGENTE REAL no canvas,
// com sprite próprio e chat nativo pelo ChatPanel. Não é wizard externo.
// Categoria no manifest: meta.
//
// Fluxo:
//  1. Humano abre chat com "arquiteto" (sempre instalado).
//  2. Conversa normal, arquiteto coleta: nome do negócio, setor, canais,
//     skills-chave, agentes desejados, tom.
//  3. Em qualquer turno humano pode pedir "gera o seed" → este módulo
//     produz plugin/seeds/custom/<slug>/ a partir de um spec JSON.

const fs   = require('fs');
const path = require('path');

const SEEDS_DIR = path.join(__dirname, '..', 'seeds');
const ARCHITECT_AGENT = 'architect';

const ARCHITECT_PROMPT = `Você é o Arquiteto — agente meta do project-mem.

Seu papel é entrevistar o humano para entender o negócio dele e desenhar um
TIME DE AGENTES customizado (um "seed"). Você:

- Faz UMA pergunta por vez. Curtas. Em tom de conversa, não de formulário.
- Nunca pede pra responder por número. Aceita resposta livre.
- Sempre confirma antes de avançar ("entendi assim, segue ou ajusto?").
- Quando tiver contexto suficiente, propõe o time: nomes, funções, 1 linha cada.
- Usa YAGNI: só cria agente que resolve dor clara.

Informações que você precisa coletar (em qualquer ordem):
1. Nome do negócio e o que ele faz em 1 frase.
2. Setor/categoria (tecnologia, atendimento, delivery, vendas, outros).
3. Canais principais (WhatsApp, Instagram, site, marketplace).
4. 3 dores atuais que um time de agentes resolveria.
5. Tom do negócio (formal, casual, técnico, emocional).

Ao final, apresente o time e espere "gera o seed" para concluir. Depois disso
o humano pode rodar /api/architect/generate via UI, que converte a conversa em
seeds/custom/<slug>/.

Lembretes:
- Se o humano começar direto pelo "gera o seed", pergunte o mínimo antes.
- Se o negócio for claramente um dos existentes (tech), diga isso antes de
  criar custom — pode ser mais barato aplicar um pack pronto.
- Glossário do projeto em PROJECT.md é referência obrigatória.
`;

function getArchitectConfig() {
  return {
    label: 'Arquiteto',
    icon: '🧠',
    category: 'meta',
    description: 'Entrevista o humano e desenha um time de agentes customizado pro negócio.',
    context_file: 'agents/architect-meta.md',
    best_practices: [],
    channels: {
      chat_required: true,
    },
  };
}

function getArchitectContext() {
  return ARCHITECT_PROMPT;
}

// Ensure the architect is present as an agent in the project's agents.json.
function ensureInstalled({ memDir }) {
  if (!fs.existsSync(memDir)) fs.mkdirSync(memDir, { recursive: true });
  const manifestPath = path.join(memDir, 'agents.json');
  let manifest;
  if (fs.existsSync(manifestPath)) {
    manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
  } else {
    manifest = { version: 1, manager: 'manager', oracle: 'oracle', agents: {} };
  }
  if (!manifest.agents) manifest.agents = {};

  const already = !!manifest.agents.architect && manifest.agents.architect.category === 'meta';
  if (!already) {
    manifest.agents.architect = getArchitectConfig();
    fs.writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + '\n');
  }
  const ctxPath = path.join(memDir, 'agents', 'architect-meta.md');
  if (!fs.existsSync(ctxPath)) {
    fs.mkdirSync(path.dirname(ctxPath), { recursive: true });
    fs.writeFileSync(ctxPath, ARCHITECT_PROMPT);
  }
  return { installed: true };
}

// Generate a custom seed from a design spec produced by architect + human.
// spec:
//   { slug, category, label, description, agents: [{ name, label, description, icon, best_practices }] }
function generateSeed(spec) {
  if (!spec || !spec.slug || !spec.category) {
    throw new Error('spec inválido: precisa slug e category');
  }
  if (!/^[a-z0-9-]+$/.test(spec.slug) || !/^[a-z0-9-]+$/.test(spec.category)) {
    throw new Error('slug e category devem ser kebab-case (a-z0-9-)');
  }
  const catDir  = path.join(SEEDS_DIR, spec.category);
  const packDir = path.join(catDir, spec.slug);

  if (!fs.existsSync(catDir)) fs.mkdirSync(catDir, { recursive: true });
  if (!fs.existsSync(path.join(catDir, 'meta.json'))) {
    fs.writeFileSync(path.join(catDir, 'meta.json'), JSON.stringify({
      label: spec.category_label || spec.category,
      description: spec.category_description || `Seeds gerados pelo Arquiteto (${spec.category}).`,
    }, null, 2) + '\n');
  }
  if (fs.existsSync(packDir)) {
    throw new Error(`seed "${spec.category}/${spec.slug}" já existe`);
  }
  fs.mkdirSync(packDir, { recursive: true });
  fs.mkdirSync(path.join(packDir, 'agents'), { recursive: true });

  const manifest = {
    version: 1,
    manager: 'manager',
    oracle: 'oracle',
    agents: {},
  };
  const packBp = new Set(spec.best_practices || []);

  for (const a of spec.agents || []) {
    if (!a.name) continue;
    const name = String(a.name).toLowerCase().replace(/[^a-z0-9_]/g, '_');
    const ctxRel = `agents/${name}.md`;
    manifest.agents[name] = {
      label: a.label || name,
      icon: a.icon || '🤖',
      category: a.category || 'specialist',
      description: a.description || '',
      context_file: ctxRel,
      best_practices: a.best_practices || [],
    };
    (a.best_practices || []).forEach(id => packBp.add(id));
    const md = a.prompt || `# ${a.label || name}\n\n${a.description || ''}\n`;
    fs.writeFileSync(path.join(packDir, ctxRel), md);
  }

  fs.writeFileSync(path.join(packDir, 'agents.json'),
    JSON.stringify(manifest, null, 2) + '\n');

  fs.writeFileSync(path.join(packDir, 'meta.json'), JSON.stringify({
    label: spec.label || spec.slug,
    description: spec.description || 'Seed customizado gerado pelo Arquiteto.',
    best_practices: [...packBp],
    generated_by: 'architect',
    generated_at: new Date().toISOString(),
  }, null, 2) + '\n');

  return {
    ok: true,
    ref: `${spec.category}/${spec.slug}`,
    dir: packDir,
    agent_count: Object.keys(manifest.agents).length,
  };
}

module.exports = {
  ARCHITECT_AGENT,
  ARCHITECT_PROMPT,
  getArchitectConfig,
  getArchitectContext,
  ensureInstalled,
  generateSeed,
};
