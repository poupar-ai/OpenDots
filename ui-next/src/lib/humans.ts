export type HumanLocation = 'core' | 'devs' | 'specialists' | 'integrators' | 'motors' | 'firefighter';

export interface HumanSkin {
  skinTone: number;
  hair: number;
  shirt: number;
  pants: number;
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
  skin?: HumanSkin | null;
  created_at: string;
  last_seen_at?: string | null;
  online?: boolean;
}

export interface HumanCreateInput {
  name: string;
  description?: string;
  skills?: string[];
  important?: string;
  notes?: string;
  offline_privacy?: { never_share: string[] };
  location?: HumanLocation | null;
  skin?: HumanSkin | null;
}

export const HUMAN_LOCATIONS: { id: HumanLocation; label: string }[] = [
  { id: 'core', label: 'Comando' },
  { id: 'devs', label: 'Devs' },
  { id: 'specialists', label: 'Equipe' },
  { id: 'integrators', label: 'Integrações' },
  { id: 'motors', label: 'Operações' },
  { id: 'firefighter', label: 'Monitoramento' },
];

const SKIN_PALETTE = [0xffd1a4, 0xf1c27d, 0xe0ac69, 0xc68642, 0x8d5524];
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
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  const j = await r.json();
  return j.human || j;
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

export async function humanHandoff(from: string, to: string, message: string): Promise<void> {
  const r = await fetch('/api/human-handoff', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ from, to, message }),
  });
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
}

export async function humanChatHistory(me: string, withHuman: string): Promise<HumanChatMsg[]> {
  const r = await fetch(
    `/api/human-chat/history?me=${encodeURIComponent(me)}&with=${encodeURIComponent(withHuman)}`,
  );
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  const j = await r.json();
  return j.messages || [];
}
