import { useEffect, useState } from 'react';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { CopyButton } from '@/components/CopyButton';
import type { AgentCfg, AgentLogSummary, AutonomyLevel, Manifest } from '@/lib/types';
import { Eye, ArrowRight, ArrowLeft, AlertCircle, Activity, Sliders, X, Save } from 'lucide-react';
import { updateAutonomy } from '@/lib/agents';

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
  const [editAuto, setEditAuto] = useState(false);

  useEffect(() => {
    setLogOpen(false);
    setLogEntries(null);
    setEditAuto(false);
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
          <CardTitle className="flex items-center gap-1.5">
            <span>{cfg.emoji || '🤖'}</span>
            {name}
            <CopyButton value={name} label="copiar nome do agente" />
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
        <div className="rounded-md border bg-indigo-50/40 p-2">
          <div className="mb-1 flex items-center justify-between">
            <div className="flex items-center gap-1 text-indigo-700">
              <Sliders className="h-3 w-3" />
              <span className="text-[10px] font-semibold uppercase tracking-wide">autonomia</span>
            </div>
            <button
              onClick={() => setEditAuto(true)}
              className="text-[10px] text-indigo-700 underline-offset-2 hover:underline"
            >
              editar
            </button>
          </div>
          {cfg.autonomy ? (
            <>
              <div className="mb-1 flex items-center gap-1.5 text-[10px]">
                <Badge variant="outline" className="uppercase">
                  {cfg.autonomy.level}
                </Badge>
                {cfg.autonomy.call_human_when?.length > 0 && (
                  <span className="text-slate-500">
                    · chama humano em {cfg.autonomy.call_human_when.length} {cfg.autonomy.call_human_when.length === 1 ? 'caso' : 'casos'}
                  </span>
                )}
              </div>
              {cfg.autonomy.call_human_when?.length > 0 && (
                <ul className="space-y-0.5 text-[10px] text-slate-700">
                  {cfg.autonomy.call_human_when.map((c, i) => (
                    <li key={i} className="flex gap-1">
                      <span className="shrink-0 text-indigo-400">·</span>
                      <span>{c}</span>
                    </li>
                  ))}
                </ul>
              )}
            </>
          ) : (
            <p className="text-[10px] text-slate-500">não configurada — usa comportamento default.</p>
          )}
        </div>
        {editAuto && (
          <AutonomyEditor
            name={name}
            cfg={cfg}
            origin={manifest?._meta?.agent_origin?.[name] || null}
            onClose={() => setEditAuto(false)}
          />
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

const LEVEL_DESC: Record<AutonomyLevel, string> = {
  low: 'escala quase tudo — só responde factual sozinho',
  medium: 'decide em tarefas claras; escala ambíguas',
  high: 'decide sozinho; só escala pelos casos listados',
};

interface AutonomyEditorProps {
  name: string;
  cfg: AgentCfg;
  origin: { category: string; slug: string } | null;
  onClose: () => void;
}

function AutonomyEditor({ name, cfg, origin, onClose }: AutonomyEditorProps) {
  const current = cfg.autonomy;
  const [level, setLevel] = useState<AutonomyLevel>(current?.level || 'medium');
  const [cases, setCases] = useState<string>(
    (current?.call_human_when || []).join('\n'),
  );
  const [propagate, setPropagate] = useState<boolean>(!!origin);
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const save = async () => {
    setErr(null);
    setSaving(true);
    try {
      const call_human_when = cases
        .split('\n')
        .map((s) => s.trim())
        .filter(Boolean);
      await updateAutonomy(name, {
        level,
        call_human_when,
        propagate_seed: propagate && !!origin,
        ...(origin ? { seed_category: origin.category, seed_slug: origin.slug } : {}),
      });
      onClose();
    } catch (e: any) {
      setErr(e?.message || 'falha ao salvar');
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/50 backdrop-blur-sm p-4">
      <div className="flex max-h-[85vh] w-full max-w-md flex-col overflow-hidden rounded-xl border bg-white shadow-2xl">
        <header className="flex items-center justify-between border-b px-4 py-3">
          <div>
            <div className="flex items-center gap-2 text-sm font-semibold">
              <Sliders className="h-4 w-4" /> Autonomia de {name}
            </div>
            <p className="text-[11px] text-muted-foreground">
              quando o agente decide sozinho e quando chama você
            </p>
          </div>
          <Button size="icon" variant="ghost" className="h-7 w-7" onClick={onClose}>
            <X className="h-4 w-4" />
          </Button>
        </header>
        <div className="soft-scroll flex-1 overflow-y-auto p-4 text-xs">
          <label className="mb-1 block text-[11px] font-semibold uppercase text-slate-600">
            Nível
          </label>
          <div className="mb-3 grid grid-cols-3 gap-1.5">
            {(['low', 'medium', 'high'] as AutonomyLevel[]).map((lv) => (
              <button
                key={lv}
                onClick={() => setLevel(lv)}
                className={`rounded border px-2 py-1.5 text-[11px] font-medium transition ${
                  level === lv
                    ? 'border-indigo-500 bg-indigo-50 text-indigo-700'
                    : 'border-slate-200 hover:bg-slate-50'
                }`}
              >
                {lv}
              </button>
            ))}
          </div>
          <p className="mb-3 rounded bg-slate-50 px-2 py-1.5 text-[11px] text-slate-600">
            {LEVEL_DESC[level]}
          </p>

          <label className="mb-1 block text-[11px] font-semibold uppercase text-slate-600">
            Sempre chamar o humano quando… <span className="text-slate-400">(uma por linha)</span>
          </label>
          <textarea
            value={cases}
            onChange={(e) => setCases(e.target.value)}
            rows={6}
            className="mb-3 w-full rounded border border-slate-200 bg-white px-2 py-1.5 font-mono text-[11px] focus:border-indigo-400 focus:outline-none"
            placeholder="decisão envolve gastar dinheiro&#10;mudança vai pra produção&#10;dois especialistas discordam"
          />

          <label className="mb-3 flex items-start gap-2 text-[11px]">
            <input
              type="checkbox"
              checked={propagate}
              disabled={!origin}
              onChange={(e) => setPropagate(e.target.checked)}
              className="mt-0.5"
            />
            <span className={origin ? 'text-slate-700' : 'text-slate-400'}>
              Propagar também ao seed {origin ? (
                <code className="rounded bg-slate-100 px-1">{origin.category}/{origin.slug}</code>
              ) : '(origem do seed desconhecida pra este agente)'}
              <br />
              <span className="text-[10px] text-slate-500">
                se marcado, a mesma mudança é gravada no template — próximos projetos já vêm com essa autonomia.
              </span>
            </span>
          </label>

          {err && (
            <div className="mb-2 rounded border border-red-200 bg-red-50 px-2 py-1.5 text-[11px] text-red-700">
              {err}
            </div>
          )}
        </div>
        <footer className="flex justify-end gap-2 border-t px-4 py-3">
          <Button size="sm" variant="outline" onClick={onClose} disabled={saving}>
            cancelar
          </Button>
          <Button size="sm" onClick={save} disabled={saving} className="gap-1.5">
            <Save className="h-3.5 w-3.5" />
            {saving ? 'salvando…' : 'salvar'}
          </Button>
        </footer>
      </div>
    </div>
  );
}
