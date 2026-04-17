export type AgentType = 'core' | 'specialist' | 'integrator' | 'motor' | 'firefighter';

export interface AgentCfg {
  type?: AgentType;
  profession?: string;
  role?: string;
  emoji?: string;
  can_talk_to?: string[];
  consults?: string[]; // legacy
  model?: string;
  context_file?: string;
  tools?: string[];
  mcps?: string[];
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

export interface AgentLogSummary {
  total: number;
  requests: number;
  responses: number;
  errors: number;
  lastErrorAt: string | null;
  lastErrorMsg: string | null;
  lastErrorHint?: string | null;
  lastDurationMs: number | null;
}

export interface AgentsData {
  manifest: Manifest;
  contexts: Record<string, string>;
  counts: Record<string, number>;
  recent: Record<string, string>;
  logs?: Record<string, AgentLogSummary>;
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
