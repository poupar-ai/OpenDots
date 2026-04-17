export type AgentType = 'core' | 'specialist' | 'integrator' | 'motor' | 'firefighter';

export interface AgentCfg {
  type?: AgentType;
  role?: string;
  emoji?: string;
  consults?: string[];
  model?: string;
  context_file?: string;
  watch?: {
    source?: string;
    query?: string;
    threshold?: number;
    window_minutes?: number;
    cooldown_seconds?: number;
    max_triages_per_hour?: number;
  };
}

export interface Manifest {
  agents: Record<string, AgentCfg>;
}

export interface AgentsData {
  manifest: Manifest;
  contexts: Record<string, string>;
  counts: Record<string, number>;
  recent: Record<string, string>;
}

export interface ServerData {
  project: { name: string; root: string };
  soulMd: string | null;
  projectMd: string | null;
  decisionsMd: string | null;
  gotchasMd: string | null;
  glossaryMd: string | null;
  codeIndexMd: string | null;
  agents: AgentsData | null;
  sessions: any[];
  observations: any[];
  decisions: any[];
  corrections: any[];
  gotchas: any[];
  stats: any;
}

export interface ChatMsg {
  ts: string;
  role: 'user' | 'agent';
  content: string;
  duration_ms?: number;
}
