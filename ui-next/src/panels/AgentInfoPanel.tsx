import { useEffect, useState } from 'react';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import type { AgentCfg, AgentLogSummary, Manifest } from '@/lib/types';
import { Eye, ArrowRight, ArrowLeft, AlertCircle, Activity } from 'lucide-react';

interface LogEntry {
  ts: string;
  type: 'request' | 'response' | 'error';
  stage?: string;
  code?: number;
  message?: string;
  stderr?: string;
  duration_ms?: number;
  response_chars?: number;
  prompt_chars?: number;
  user_chars?: number;
}

interface Props {
  name: string;
  cfg: AgentCfg;
  context: string;
  count?: number;
  lastInvoked?: string;
  manifest?: Manifest | null;
  logSummary?: AgentLogSummary;
  onSelect?: (name: string) => void;
}

function edges(c: AgentCfg): string[] {
  return c.can_talk_to || c.consults || [];
}

function LogRow({ e }: { e: LogEntry }) {
  const color =
    e.type === 'error'
      ? 'bg-red-100 text-red-700'
      : e.type === 'response'
        ? 'bg-emerald-100 text-emerald-700'
        : 'bg-blue-100 text-blue-700';
  const detail =
    e.type === 'error'
      ? e.message || e.stderr || `exit ${e.code ?? '?'}`
      : e.type === 'response'
        ? `${e.response_chars ?? 0} chars · ${e.duration_ms ?? 0}ms`
        : `prompt ${e.prompt_chars ?? 0} chars`;
  return (
    <div className="flex items-start gap-1.5 text-[10px] leading-tight">
      <span className="shrink-0 text-slate-400">{e.ts.slice(11, 19)}</span>
      <span className={`shrink-0 rounded px-1 font-semibold uppercase ${color}`}>{e.type}</span>
      <span className="min-w-0 flex-1 truncate text-slate-700">{detail}</span>
    </div>
  );
}

export function AgentInfoPanel({
  name,
  cfg,
  context,
  count,
  lastInvoked,
  manifest,
  logSummary,
  onSelect,
}: Props) {
  const type = cfg.type || 'specialist';
  const outgoing = edges(cfg);
  const incoming = manifest
    ? Object.entries(manifest.agents)
        .filter(([n, a]) => n !== name && edges(a).includes(name))
        .map(([n]) => n)
    : [];
  const [logOpen, setLogOpen] = useState(false);
  const [logEntries, setLogEntries] = useState<LogEntry[] | null>(null);
  const [logLoading, setLogLoading] = useState(false);

  useEffect(() => {
    setLogOpen(false);
    setLogEntries(null);
  }, [name]);

  const loadLog = async () => {
    if (logEntries) return;
    setLogLoading(true);
    try {
      const r = await fetch(`/api/agents/log?agent=${encodeURIComponent(name)}&limit=40`);
      const data = await r.json();
      setLogEntries(Array.isArray(data.entries) ? data.entries : []);
    } catch {
      setLogEntries([]);
    } finally {
      setLogLoading(false);
    }
  };

  return (
    <Card>
      <CardHeader className="pb-2">
        <div className="flex items-center justify-between">
          <CardTitle className="flex items-center gap-2">
            <span>{cfg.emoji || '🤖'}</span>
            {name}
          </CardTitle>
          <Badge variant="outline">{type}</Badge>
        </div>
        {cfg.role && <p className="text-xs text-muted-foreground">{cfg.role}</p>}
      </CardHeader>
      <CardContent className="space-y-2 pt-0 text-xs">
        {cfg.model && (
          <div className="flex items-center justify-between">
            <span className="text-muted-foreground">modelo</span>
            <span className="font-mono">{cfg.model}</span>
          </div>
        )}
        {cfg.watch && (
          <div className="flex items-start gap-1 rounded-md bg-red-50 px-2 py-1.5 text-red-700">
            <Eye className="mt-0.5 h-3 w-3 shrink-0" />
            <div className="min-w-0">
              <div className="font-mono text-[10px] truncate">{cfg.watch.query}</div>
              <div className="text-[10px] text-red-500">
                threshold {cfg.watch.threshold}/{cfg.watch.window_minutes}min
              </div>
            </div>
          </div>
        )}
        {outgoing.length > 0 && (
          <div>
            <div className="mb-1 flex items-center gap-1 text-orange-600">
              <ArrowRight className="h-3 w-3" />
              <span className="text-[10px] font-semibold uppercase tracking-wide">fala com</span>
            </div>
            <div className="flex flex-wrap gap-1">
              {outgoing.map((c) => (
                <button
                  key={c}
                  onClick={() => onSelect?.(c)}
                  className="rounded-full border border-orange-200 bg-orange-50 px-2 py-0.5 text-[10px] font-medium text-orange-700 hover:bg-orange-100"
                >
                  {c}
                </button>
              ))}
            </div>
          </div>
        )}
        {incoming.length > 0 && (
          <div>
            <div className="mb-1 flex items-center gap-1 text-blue-600">
              <ArrowLeft className="h-3 w-3" />
              <span className="text-[10px] font-semibold uppercase tracking-wide">recebe de</span>
            </div>
            <div className="flex flex-wrap gap-1">
              {incoming.map((c) => (
                <button
                  key={c}
                  onClick={() => onSelect?.(c)}
                  className="rounded-full border border-blue-200 bg-blue-50 px-2 py-0.5 text-[10px] font-medium text-blue-700 hover:bg-blue-100"
                >
                  {c}
                </button>
              ))}
            </div>
          </div>
        )}
        {(count !== undefined || lastInvoked) && (
          <div className="flex items-center justify-between border-t pt-2 text-muted-foreground">
            <span>invocações: {count ?? 0}</span>
            {lastInvoked && <span className="text-[10px]">{lastInvoked.slice(0, 10)}</span>}
          </div>
        )}
        {logSummary && (
          <div className="rounded-md border bg-slate-50/70 p-2">
            <div className="mb-1 flex items-center justify-between">
              <div className="flex items-center gap-1 text-slate-700">
                <Activity className="h-3 w-3" />
                <span className="text-[10px] font-semibold uppercase tracking-wide">log</span>
              </div>
              <div className="flex items-center gap-1.5 text-[10px]">
                <span className="text-slate-600">{logSummary.requests} req</span>
                <span className={logSummary.errors > 0 ? 'text-red-600 font-semibold' : 'text-slate-400'}>
                  {logSummary.errors} err
                </span>
                {typeof logSummary.lastDurationMs === 'number' && (
                  <span className="text-slate-500">{logSummary.lastDurationMs}ms</span>
                )}
              </div>
            </div>
            {logSummary.lastErrorMsg && (
              <div className="flex items-start gap-1 rounded bg-red-50 px-1.5 py-1 text-red-700">
                <AlertCircle className="mt-0.5 h-3 w-3 shrink-0" />
                <div className="min-w-0 flex-1">
                  <div className="text-[10px] text-red-500">
                    último erro {logSummary.lastErrorAt ? logSummary.lastErrorAt.slice(11, 19) : ''}
                  </div>
                  <div className="truncate font-mono text-[10px]">{logSummary.lastErrorMsg}</div>
                </div>
              </div>
            )}
            <button
              onClick={() => {
                setLogOpen((v) => !v);
                if (!logOpen) loadLog();
              }}
              className="mt-1 w-full rounded border bg-white px-2 py-1 text-[10px] font-medium text-slate-600 hover:bg-slate-100"
            >
              {logOpen ? 'ocultar' : 'ver'} eventos
            </button>
            {logOpen && (
              <div className="soft-scroll mt-1 max-h-48 space-y-0.5 overflow-y-auto pr-1">
                {logLoading && <div className="text-[10px] text-slate-500">carregando…</div>}
                {!logLoading && logEntries && logEntries.length === 0 && (
                  <div className="text-[10px] text-slate-500">sem eventos ainda.</div>
                )}
                {!logLoading &&
                  logEntries &&
                  [...logEntries].reverse().map((e, i) => <LogRow key={i} e={e} />)}
              </div>
            )}
          </div>
        )}
        {context && (
          <details className="rounded-md border bg-muted/30 p-2">
            <summary className="cursor-pointer text-[11px] font-medium">mini-contexto</summary>
            <pre className="mt-2 max-h-48 overflow-auto whitespace-pre-wrap text-[10px]">
              {context}
            </pre>
          </details>
        )}
      </CardContent>
    </Card>
  );
}
