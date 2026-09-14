// Client TypeScript pras rotas /api/cron do poupar-http.

export interface CronJob {
  id: string;
  owner: string;
  target: string;
  message: string;
  schedule: string | null;
  run_at_ms: number | null;
  one_shot: boolean;
  created_at_ms: number;
  active: boolean;
  next_run_ms: number | null;
  last_run_ms: number | null;
}

export interface CreateCronInput {
  /** Agente que dispara o job. UI passa via body (sem Authorization). */
  owner: string;
  target: string;
  message: string;
  /** Recorrente (mutuamente exclusivo com run_in_seconds/run_at_ms). */
  schedule?: string;
  /** One-shot: dispara uma vez daqui a N segundos, auto-desativa. */
  run_in_seconds?: number;
  /** One-shot: timestamp absoluto (ms desde epoch). */
  run_at_ms?: number;
}

export async function listCronJobs(includeInactive = false): Promise<CronJob[]> {
  const qs = includeInactive ? '?all=1' : '';
  const r = await fetch(`/api/cron${qs}`);
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  return r.json();
}

export async function createCronJob(input: CreateCronInput): Promise<CronJob> {
  const r = await fetch('/api/cron', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(input),
  });
  const j = await r.json();
  if (!r.ok) throw new Error(j.error || `HTTP ${r.status}`);
  return j;
}

export async function deleteCronJob(id: string): Promise<void> {
  const r = await fetch(`/api/cron/${encodeURIComponent(id)}`, { method: 'DELETE' });
  if (!r.ok) {
    const j = await r.json().catch(() => ({}));
    throw new Error(j.error || `HTTP ${r.status}`);
  }
}

/** Atalhos legíveis pra dropdown da UI. */
export const SCHEDULE_PRESETS: { label: string; expr: string }[] = [
  { label: 'A cada 30 segundos', expr: '*/30 * * * * *' },
  { label: 'A cada minuto', expr: '0 * * * * *' },
  { label: 'A cada 5 minutos', expr: '0 */5 * * * *' },
  { label: 'A cada 15 minutos', expr: '0 */15 * * * *' },
  { label: 'A cada hora (cheia)', expr: '@hourly' },
  { label: 'Todo dia às 9h', expr: '0 0 9 * * *' },
  { label: 'Dias úteis às 9h', expr: '0 0 9 * * 1-5' },
  { label: 'Toda segunda 9h', expr: '0 0 9 * * 1' },
  { label: 'Todo dia (meia-noite)', expr: '@daily' },
  { label: 'Toda semana (domingo 0h)', expr: '@weekly' },
];
