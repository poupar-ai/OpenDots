// Motor de detecção de intent destrutivo — executa server-side antes do agente.
//
// Objetivo: não depender da IA pra barrar pedidos tipo "apague tudo", "delete
// o repositório", "rm -rf", "drop database". Toda mensagem (user→agente,
// agente→agente via handoff) passa por `detect()`. Se casar, o fluxo normal
// é interceptado: agente de destino não é invocado, internal_firefighter é
// acionado com contexto completo.
//
// Patterns CORE são imutáveis (não editáveis via UI). Patterns USER ficam em
// <memDir>/destructive_patterns.json e podem ser gerenciados na UI.

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

// ── CORE: patterns fixos, nunca removíveis via UI ────────────────────────────
// Cada entry: { id, label, regex, severity, origin: 'core' }
// Regexes cobrem PT + EN. Ordem importa pouco (detect roda todos).
const CORE_PATTERNS = [
  {
    id: 'core.apagar_tudo',
    label: 'Apagar tudo / todo o projeto / repositório / código',
    regex: /\b(apag\w+|delet\w+|remov\w+|limp\w+|destru\w+|deleta)\b[^.!?\n]{0,40}\b(tudo|todos? os arquivos?|todo o (projeto|reposit[óo]rio|c[óo]digo|sistema)|o reposit[óo]rio|o projeto|o c[óo]digo|a branch|o banco|a base( de dados)?|as bases?|as mensagens?|as mem[óo]rias?)/i,
    severity: 'critical',
  },
  {
    id: 'core.delete_everything_en',
    label: 'Delete everything / all / repo / project (EN)',
    regex: /\b(delete|remove|wipe|drop|nuke|destroy|purge)\b[^.!?\n]{0,40}\b(everything|all( files| data)?|the (repo|repository|project|code(base)?|branch|database|db))\b/i,
    severity: 'critical',
  },
  {
    id: 'core.rm_rf',
    label: 'rm -rf (comando shell destrutivo)',
    regex: /\brm\s+-[rRf]{1,3}\b/,
    severity: 'critical',
  },
  {
    id: 'core.git_reset_hard',
    label: 'git reset --hard',
    regex: /\bgit\s+reset\s+--hard\b/i,
    severity: 'critical',
  },
  {
    id: 'core.git_clean_force',
    label: 'git clean -f / -fd / -fdx',
    regex: /\bgit\s+clean\s+-[fdxFDX]{1,3}\b/i,
    severity: 'critical',
  },
  {
    id: 'core.git_push_force',
    label: 'git push --force / -f',
    regex: /\bgit\s+push\s+(--force\b|-f\b)/i,
    severity: 'high',
  },
  {
    id: 'core.git_branch_delete',
    label: 'git branch -D (deletar branch forçado)',
    regex: /\bgit\s+branch\s+-D\b/i,
    severity: 'high',
  },
  {
    id: 'core.drop_database',
    label: 'DROP DATABASE / TABLE / SCHEMA',
    regex: /\bDROP\s+(DATABASE|TABLE|SCHEMA|INDEX)\b/i,
    severity: 'critical',
  },
  {
    id: 'core.truncate_table',
    label: 'TRUNCATE TABLE',
    regex: /\bTRUNCATE\s+TABLE\b/i,
    severity: 'critical',
  },
  {
    id: 'core.delete_from_no_where',
    label: 'DELETE FROM sem WHERE',
    regex: /\bDELETE\s+FROM\s+\w+\s*(;|$)/i,
    severity: 'critical',
  },
  {
    id: 'core.format_disk',
    label: 'format / mkfs (formatar disco)',
    regex: /\b(mkfs|format)\s+(\/dev|c:|disk|hd|ssd)/i,
    severity: 'critical',
  },
  {
    id: 'core.dd_of_dev',
    label: 'dd if=... of=/dev/...',
    regex: /\bdd\s+.*\bof=\/dev\//i,
    severity: 'critical',
  },
  {
    id: 'core.chmod_777_root',
    label: 'chmod 777 em raiz',
    regex: /\bchmod\s+-?R?\s*777\s+\/(\s|$)/i,
    severity: 'high',
  },
  {
    id: 'core.shutdown',
    label: 'shutdown / poweroff / halt / reboot',
    regex: /\b(shutdown\s+-h|poweroff|halt|reboot\s+now)\b/i,
    severity: 'high',
  },
];

Object.freeze(CORE_PATTERNS);
CORE_PATTERNS.forEach((p) => {
  p.origin = 'core';
  Object.freeze(p);
});

// ── USER: patterns editáveis via UI ──────────────────────────────────────────
function userPatternsPath(memDir) {
  return path.join(memDir, 'destructive_patterns.json');
}

function loadUserConfig(memDir) {
  try {
    const p = userPatternsPath(memDir);
    if (!fs.existsSync(p)) return { patterns: [], disabledCore: [] };
    const raw = JSON.parse(fs.readFileSync(p, 'utf8'));
    return {
      patterns: Array.isArray(raw.patterns) ? raw.patterns : [],
      disabledCore: Array.isArray(raw.disabled_core) ? raw.disabled_core : [],
    };
  } catch {
    return { patterns: [], disabledCore: [] };
  }
}

function loadUserPatterns(memDir) {
  return loadUserConfig(memDir).patterns;
}

function saveUserPatterns(memDir, patterns) {
  const current = loadUserConfig(memDir);
  const normalized = patterns.map((pt) => ({
    id: pt.id || ('user.' + crypto.randomBytes(6).toString('hex')),
    label: String(pt.label || '').slice(0, 200),
    text: String(pt.text || '').slice(0, 400),
    kind: pt.kind === 'regex' ? 'regex' : 'literal',
    severity: ['critical', 'high', 'medium'].includes(pt.severity) ? pt.severity : 'high',
    enabled: pt.enabled !== false,
    created_at: pt.created_at || new Date().toISOString(),
  })).filter((pt) => pt.label && pt.text);
  const p = userPatternsPath(memDir);
  fs.writeFileSync(p, JSON.stringify({
    patterns: normalized,
    disabled_core: current.disabledCore,
  }, null, 2));
  return normalized;
}

// Liga/desliga um pattern CORE. IDs desconhecidos são ignorados.
function setCoreDisabled(memDir, coreId, disabled) {
  const validIds = new Set(CORE_PATTERNS.map((p) => p.id));
  if (!validIds.has(coreId)) throw new Error(`unknown core pattern id: ${coreId}`);
  const current = loadUserConfig(memDir);
  const set = new Set(current.disabledCore);
  if (disabled) set.add(coreId); else set.delete(coreId);
  const p = userPatternsPath(memDir);
  fs.writeFileSync(p, JSON.stringify({
    patterns: current.patterns,
    disabled_core: Array.from(set),
  }, null, 2));
  return Array.from(set);
}

// Converte literal+wildcard em regex segura. Asterisco vira `.*`, resto é escapado.
// Regex kind é usado direto, com flag 'i'. Entradas inválidas retornam null.
function compileUserPattern(pt) {
  if (!pt || !pt.enabled) return null;
  try {
    if (pt.kind === 'regex') {
      return new RegExp(pt.text, 'i');
    }
    // Literal com suporte a *
    const escaped = pt.text.replace(/[.+?^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*');
    return new RegExp(`\\b${escaped}\\b`, 'i');
  } catch {
    return null;
  }
}

// ── API pública ──────────────────────────────────────────────────────────────
function listPatterns(memDir) {
  const cfg = loadUserConfig(memDir);
  const disabledSet = new Set(cfg.disabledCore);
  const user = cfg.patterns.map((p) => ({
    id: p.id,
    label: p.label,
    text: p.text,
    kind: p.kind,
    severity: p.severity,
    enabled: p.enabled,
    origin: 'user',
    created_at: p.created_at,
  }));
  const core = CORE_PATTERNS.map((p) => ({
    id: p.id,
    label: p.label,
    text: p.regex.source,
    kind: 'regex',
    severity: p.severity,
    enabled: !disabledSet.has(p.id),
    origin: 'core',
    readonly: true, // não pode editar texto/severidade, mas pode desabilitar
  }));
  return { core, user };
}

function detect(memDir, text) {
  if (!text || typeof text !== 'string') return { matched: false, hits: [] };
  const cfg = loadUserConfig(memDir);
  const disabledSet = new Set(cfg.disabledCore);
  const hits = [];
  // CORE (pula os desabilitados pelo usuário)
  for (const p of CORE_PATTERNS) {
    if (disabledSet.has(p.id)) continue;
    const m = text.match(p.regex);
    if (m) {
      hits.push({
        id: p.id, label: p.label, origin: 'core',
        severity: p.severity, match: m[0].slice(0, 120),
      });
    }
  }
  // USER
  for (const p of cfg.patterns) {
    const re = compileUserPattern(p);
    if (!re) continue;
    const m = text.match(re);
    if (m) {
      hits.push({
        id: p.id, label: p.label, origin: 'user',
        severity: p.severity, match: m[0].slice(0, 120),
      });
    }
  }
  return { matched: hits.length > 0, hits };
}

function testPattern(memDir, text) {
  return detect(memDir, text);
}

module.exports = {
  CORE_PATTERNS,
  listPatterns,
  loadUserPatterns,
  saveUserPatterns,
  setCoreDisabled,
  detect,
  testPattern,
};
