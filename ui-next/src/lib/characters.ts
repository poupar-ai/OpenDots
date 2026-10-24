import type { AgentType } from '@/lib/types';
import robotUrl from '@/assets/characters/robot.png';
import sproutUrl from '@/assets/characters/sprout.png';
import foxUrl from '@/assets/characters/fox.png';
import cloudUrl from '@/assets/characters/cloud.png';

export type CharacterId = 'robot' | 'sprout' | 'fox' | 'cloud';

export const CHARACTERS: { id: CharacterId; url: string; label: string }[] = [
  { id: 'robot', url: robotUrl, label: 'robô' },
  { id: 'sprout', url: sproutUrl, label: 'broto' },
  { id: 'fox', url: foxUrl, label: 'raposa' },
  { id: 'cloud', url: cloudUrl, label: 'nuvem' },
];

// Personagem por tipo de agente.
const BY_TYPE: Record<AgentType, CharacterId> = {
  core: 'robot',
  specialist: 'sprout',
  integrator: 'cloud',
  motor: 'robot',
  firefighter: 'fox',
};

export function characterFor(type: AgentType): { id: CharacterId; url: string } {
  const id = BY_TYPE[type] || 'sprout';
  return CHARACTERS.find((c) => c.id === id)!;
}
