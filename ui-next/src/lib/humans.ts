import { setHumanToken, getHumanToken } from './session';

export type HumanLocation = 'core' | 'devs' | 'specialists' | 'integrators' | 'motors' | 'firefighter';
export type HumanGender = 'male' | 'female' | 'other';

export type HumanOutfit = 'pants' | 'dress';

export interface HumanSkin {
  skinTone: number;
  hair: number;
  shirt: number;
  pants: number;
  outfit?: HumanOutfit;
}

export interface HumanPosition {
  x: number;
  y: number;
}

export interface HumanUiPrefs {
  chatCollapsed?: boolean;
}

export interface HumanProfile {
  id: string;
  name: string;
  description?: string;
  skills: string[];
  important?: string;
  notes?: string;
  offline_privacy?: { never_share: string[] };
  location?: HumanLocation | null;
  gender?: HumanGender | null;
  skin?: HumanSkin | null;
  position?: HumanPosition | null;
  ui_prefs?: HumanUiPrefs | null;
  created_at: string;
  last_seen_at?: string | null;
  online?: boolean;
}

export interface HumanCreateInput {
  name: string;
  /** Senha do humano — backend guarda só o hash (PBKDF2) no cofre. */
  password?: string;
  email?: string;
  description?: string;
  skills?: string[];
  important?: string;
  notes?: string;
  offline_privacy?: { never_share: string[] };
  location?: HumanLocation | null;
  gender?: HumanGender | null;
  skin?: HumanSkin | null;
  position?: HumanPosition | null;
  ui_prefs?: HumanUiPrefs | null;
}

export const HUMAN_LOCATIONS: { id: HumanLocation; label: string }[] = [
  { id: 'core', label: 'Controle' },
  { id: 'devs', label: 'Devs' },
  { id: 'specialists', label: 'Equipe' },
  { id: 'integrators', label: 'Integrações' },
  { id: 'motors', label: 'Operações' },
  { id: 'firefighter', label: 'Monitoramento' },
];

// Paleta de tons de pele — categorias reais (IBGE) + opções fantasia no final.
// Usuário escolhe na UI; ordem importa pra apresentação.
export const SKIN_TONES: { id: string; label: string; hex: number; fantasy?: boolean }[] = [
  { id: 'branco',      label: 'Branco',      hex: 0xffe0d1 },
  { id: 'amarelo',     label: 'Amarelo',     hex: 0xf1c27d },
  { id: 'pardo',       label: 'Pardo',       hex: 0xc68642 },
  { id: 'cafe-claro',  label: 'Café claro',  hex: 0x9a6b3f },
  { id: 'cafe-escuro', label: 'Café escuro', hex: 0x5d3a1a },
  { id: 'preto',       label: 'Preto',       hex: 0x3a2416 },
  { id: 'rosa',        label: 'Rosa',        hex: 0xffb6c1, fantasy: true },
  { id: 'verde',       label: 'Verde',       hex: 0x86d18b, fantasy: true },
  { id: 'vermelho',    label: 'Vermelho',    hex: 0xe06666, fantasy: true },
  { id: 'prata',       label: 'Prata',       hex: 0xc0c0c0, fantasy: true },
];

const SKIN_PALETTE = SKIN_TONES.map((t) => t.hex);
const HAIR_PALETTE = [0x1f2937, 0x44403c, 0x78350f, 0xb45309, 0xeab308, 0x6b7280];
const SHIRT_PALETTE = [0x3b82f6, 0xef4444, 0x10b981, 0x8b5cf6, 0xf59e0b, 0xec4899, 0x0ea5e9];
const PANTS_PALETTE = [0x1f2937, 0x0f172a, 0x78350f, 0x334155, 0x4b5563];

export function randomSkin(): HumanSkin {
  const pick = <T,>(arr: T[]): T => arr[Math.floor(Math.random() * arr.length)];
  return {
    skinTone: pick(SKIN_PALETTE),
    hair: pick(HAIR_PALETTE),
    shirt: pick(SHIRT_PALETTE),
    pants: pick(PANTS_PALETTE),
  };
}

export interface HumanChatMsg {
  ts: string;
  from: string;
  to: string;
  content: string;
  shadow?: boolean;
}

export async function listHumans(): Promise<HumanProfile[]> {
  const r = await fetch('/api/humans');
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  const j = await r.json();
  return j.humans || [];
}

export async function createHuman(input: HumanCreateInput): Promise<HumanProfile> {
  const r = await fetch('/api/humans', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(input),
  });
  if (!r.ok) {
    const j = await r.json().catch(() => ({}));
    throw new Error(j.error || `HTTP ${r.status}`);
  }
  const j = await r.json();
  if (j.token) setHumanToken(j.token); // já loga: token de sessão emitido na criação
  return j.human || j;
}

/** Login: valida senha (hash no cofre) e guarda o token de sessão. */
export async function humanLogin(id: string, password: string): Promise<void> {
  const r = await fetch(`/api/humans/${encodeURIComponent(id)}/login`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ password }),
  });
  if (!r.ok) {
    const j = await r.json().catch(() => ({}));
    throw new Error(j.error || `HTTP ${r.status}`);
  }
  const j = await r.json();
  setHumanToken(j.token || null);
}

export async function getHuman(id: string): Promise<HumanProfile> {
  const r = await fetch(`/api/humans/${encodeURIComponent(id)}`);
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  const j = await r.json();
  return j.human || j;
}

export async function updateHuman(id: string, patch: Partial<HumanCreateInput>): Promise<HumanProfile> {
  const r = await fetch(`/api/humans/${encodeURIComponent(id)}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(patch),
  });
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  const j = await r.json();
  return j.human || j;
}

export async function heartbeatHuman(id: string): Promise<void> {
  await fetch(`/api/humans/${encodeURIComponent(id)}/heartbeat`, { method: 'POST' });
}

export async function deleteHuman(id: string): Promise<void> {
  const r = await fetch(`/api/humans/${encodeURIComponent(id)}`, { method: 'DELETE' });
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
}

export async function humanHandoff(from: string, to: string, message: string): Promise<void> {
  const token = getHumanToken();
  const r = await fetch('/api/human-handoff', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
    body: JSON.stringify({ from, to, message }),
  });
  if (!r.ok) {
    const j = await r.json().catch(() => ({}));
    throw new Error(j.error || `HTTP ${r.status}`);
  }
}

export interface AgentChatSummary {
  agent: string;
  count: number;
  last_ts: string | null;
  last_preview: string;
  last_role: 'user' | 'agent' | null;
}

export interface HumanPeerChatSummary {
  peer_id: string;
  peer_name: string;
  count: number;
  last_ts: string | null;
  last_preview: string;
  last_from_me: boolean;
}

export interface HumanChatsSummary {
  human_id: string;
  agent_chats: AgentChatSummary[];
  human_chats: HumanPeerChatSummary[];
}

export async function humanChatsSummary(id: string): Promise<HumanChatsSummary> {
  const r = await fetch(`/api/humans/${encodeURIComponent(id)}/chats`);
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  return r.json();
}

export async function humanChatHistory(_me: string, withHuman: string): Promise<HumanChatMsg[]> {
  // `me` é derivado do token no backend (segurança) — não confia no query.
  const token = getHumanToken();
  const r = await fetch(`/api/human-chat/history?with=${encodeURIComponent(withHuman)}`, {
    headers: token ? { Authorization: `Bearer ${token}` } : {},
  });
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  const j = await r.json();
  return j.messages || [];
}

/** Limpa a conversa entre o humano logado (token) e `withHuman`. Não apaga
 * o humano. Só funciona pra conversas das quais você participa. */
export async function clearHumanChat(withHuman: string): Promise<void> {
  const token = getHumanToken();
  const r = await fetch('/api/human-chat/clear', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
    body: JSON.stringify({ with: withHuman }),
  });
  if (!r.ok) {
    const j = await r.json().catch(() => ({}));
    throw new Error(j.error || `HTTP ${r.status}`);
  }
}
