import { useEffect, useState } from 'react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Modal, Stack, Group, Text, Textarea, Chip, ActionIcon, Badge as UiBadge, Button as UiButton } from '@/ui';
import { useMediaQuery } from '@/hooks/useMediaQuery';
import type { RegisteredMcp } from '@/lib/mcps';
import { McpForm } from '@/panels/McpForm';
import { CopyButton } from '@/components/CopyButton';
import { AgentFace } from '@/components/AgentFace';
import { cn } from '@/lib/utils';
import type { AgentCfg, AgentLogSummary, AutonomyLevel, Manifest } from '@/lib/types';
import {
  Eye, ArrowRight, ArrowLeft, AlertCircle, Activity, Sliders, X, Save, Cpu, Sprout,
  FileText, Pencil, Plug, Plus, Trash2, Hammer, Terminal, FilePen, ChevronDown,
  Zap, TrendingUp, Clock, Shield, ShieldAlert, ShieldCheck, KeyRound, Lock,
} from 'lucide-react';
import {
  updateAutonomy, updateModel, updateAgentContext, updateAgentMcps, listMcps, deleteAgent,
  updateAgentScope, updateAgentVault,
} from '@/lib/agents';
import { getVaultStatus, listVaultKeys } from '@/lib/vault';

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

interface WorkEntry {
  ts: string;
  type: 'file_edit' | 'bash' | 'agent_call';
  agent_id?: string;
  session_id?: string;
  file?: string;
  cmd?: string;
  exit?: number;
  output_summary?: string;
  diff?: { ins?: number; del?: number; preview?: string; tracked?: boolean; binary?: boolean };
  agent?: string;
  description?: string;
  prompt?: string;
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
  onDeleted?: (name: string) => void;
}

function edges(c: AgentCfg): string[] {
  return c.can_talk_to || c.consults || [];
}

function fmtRelativeDate(ts: string): string {
  try {
    const d = new Date(ts);
    const now = Date.now();
    const diff = Math.floor((now - d.getTime()) / 1000);
    if (diff < 60) return 'agora';
    if (diff < 3600) return `${Math.floor(diff / 60)}m atrás`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h atrás`;
    if (diff < 604800) return `${Math.floor(diff / 86400)}d atrás`;
    return ts.slice(0, 10);
  } catch {
    return ts.slice(0, 10);
  }
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
  onDeleted,
}: Props) {
  const type = cfg.type || 'specialist';
  const outgoing = edges(cfg);
  const incoming = manifest
    ? Object.entries(manifest.agents)
        .filter(([n, a]) => n !== name && edges(a).includes(name))
        .map(([n]) => n)
    : [];
  const [activityOpen, setActivityOpen] = useState(false);
  const [activityTab, setActivityTab] = useState<'log' | 'work'>('log');
  const [logEntries, setLogEntries] = useState<LogEntry[] | null>(null);
  const [logLoading, setLogLoading] = useState(false);
  const [workEntries, setWorkEntries] = useState<WorkEntry[] | null>(null);
  const [workLoading, setWorkLoading] = useState(false);
  const [editAuto, setEditAuto] = useState(false);
  const [editModel, setEditModel] = useState(false);
  const [editContext, setEditContext] = useState(false);
  const [editMcps, setEditMcps] = useState(false);
  const [editScope, setEditScope] = useState(false);
  const [editVault, setEditVault] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [deleteErr, setDeleteErr] = useState<string | null>(null);
  // Trava visual dos nomes de secret quando o cofre está fechado.
  // `null` = ainda não sabemos o status → mascarar por segurança (evita flash).
  const [vaultLocked, setVaultLocked] = useState<boolean | null>(null);

  useEffect(() => {
    let alive = true;
    const poll = () => {
      getVaultStatus()
        .then((s) => { if (alive) setVaultLocked(s.initialized ? s.locked : false); })
        .catch(() => { if (alive) setVaultLocked(true); });
    };
    poll();
    const id = setInterval(poll, 10_000);
    return () => { alive = false; clearInterval(id); };
  }, []);

  const maskIfLocked = (s: string): string => (vaultLocked === false ? s : '••••••••');

  useEffect(() => {
    setActivityOpen(false);
    setLogEntries(null);
    setWorkEntries(null);
    setEditAuto(false);
    setEditModel(false);
    setEditContext(false);
    setEditMcps(false);
    setEditScope(false);
    setEditVault(false);
    setDeleteErr(null);
  }, [name]);

  const isProtected =
    !!manifest && (name === (manifest as any).manager || name === (manifest as any).oracle);

  const handleDelete = async () => {
    if (isProtected) return;
    if (!confirm(`Remover o agente "${name}"? Isso apaga manifest + arquivo de mini-contexto.`)) return;
    setDeleting(true);
    setDeleteErr(null);
    try {
      await deleteAgent(name);
      onDeleted?.(name);
    } catch (e: any) {
      setDeleteErr(e?.message || 'falha ao remover');
    } finally {
      setDeleting(false);
    }
  };

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

  const loadWork = async () => {
    if (workEntries) return;
    setWorkLoading(true);
    try {
      const r = await fetch(`/api/agents/work?agent=${encodeURIComponent(name)}&limit=100`);
      const data = await r.json();
      setWorkEntries(Array.isArray(data.entries) ? data.entries : []);
    } catch {
      setWorkEntries([]);
    } finally {
      setWorkLoading(false);
    }
  };

  const origin = manifest?._meta?.agent_origin?.[name];
  const errorCount = logSummary?.errors ?? 0;
  const lastMs = logSummary?.lastDurationMs;

  return (
    <div className="space-y-3">
      <div className="overflow-hidden rounded-xl border bg-white shadow-sm">
        <div className="flex items-start gap-3 p-4">
          <AgentFace
            type={type}
            profession={cfg.profession}
            size={56}
          />
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-1.5">
              <h3 className="truncate text-sm font-semibold leading-tight">{name}</h3>
              <CopyButton value={name} label="copiar nome do agente" />
            </div>
            {cfg.role && (
              <p className="mt-0.5 line-clamp-2 text-xs leading-snug text-muted-foreground">
                {cfg.role}
              </p>
            )}
            <div className="mt-1.5 flex flex-wrap items-center gap-1">
              <Badge variant="outline" className="text-xs">{type}</Badge>
              {cfg.autonomy && <AutonomyBadge level={cfg.autonomy.level} />}
              {cfg.watch && (
                <span className="inline-flex items-center gap-0.5 rounded-full border border-red-200 bg-red-50 px-1.5 py-0 text-xs font-semibold uppercase tracking-wide text-red-700">
                  <Eye className="h-2.5 w-2.5" /> watch
                </span>
              )}
            </div>
            {origin && (
              <p className="mt-1 flex items-center gap-1 text-xs text-slate-400">
                <Sprout className="h-3 w-3" />
                <code className="truncate font-mono">{origin.category}/{origin.slug}</code>
              </p>
            )}
          </div>
        </div>

        <div className="grid grid-cols-3 divide-x border-t bg-slate-50/50">
          <MetricCell
            icon={<Zap className="h-3 w-3" />}
            label="invocações"
            value={count ?? 0}
          />
          <MetricCell
            icon={<AlertCircle className="h-3 w-3" />}
            label="erros"
            value={errorCount}
            tone={errorCount > 0 ? 'danger' : 'default'}
          />
          <MetricCell
            icon={<Clock className="h-3 w-3" />}
            label={lastMs != null ? 'últ. ms' : 'último'}
            value={lastMs != null ? `${lastMs}` : lastInvoked ? fmtRelativeDate(lastInvoked) : '—'}
            small={lastMs == null}
          />
        </div>
      </div>

      <div className="overflow-hidden rounded-xl border bg-white shadow-sm">
        <SectionHeader icon={<Sliders className="h-4 w-4" />} label="Configuração" />
        <div className="divide-y">
          <ConfigRow
            icon={<Cpu className="h-3.5 w-3.5" />}
            label="modelo"
            value={cfg.model || <em className="text-slate-400">default</em>}
            onEdit={() => setEditModel(true)}
          />
          <ConfigRow
            icon={<Plug className="h-3.5 w-3.5" />}
            label="mcps"
            value={
              cfg.mcps && cfg.mcps.length > 0 ? (
                <div className="flex flex-wrap items-center justify-end gap-1">
                  {cfg.mcps.slice(0, 3).map((m) => (
                    <span
                      key={m}
                      className="rounded-md border border-violet-200 bg-violet-50 px-1.5 py-0 font-mono text-xs text-violet-700"
                    >
                      {m}
                    </span>
                  ))}
                  {cfg.mcps.length > 3 && (
                    <span className="text-xs text-slate-400">+{cfg.mcps.length - 3}</span>
                  )}
                </div>
              ) : (
                <em className="text-slate-400">nenhum</em>
              )
            }
            onEdit={() => setEditMcps(true)}
          />
          <ConfigRow
            icon={<KeyRound className="h-3.5 w-3.5" />}
            label="cofre"
            value={
              cfg.secrets && cfg.secrets.length > 0 ? (
                <div className="flex flex-wrap items-center justify-end gap-1">
                  {cfg.secrets.slice(0, 3).map((s) => (
                    <span
                      key={s}
                      className="flex items-center gap-1 rounded-md border border-amber-200 bg-amber-50 px-1.5 py-0.5 font-mono text-xs text-amber-700"
                      title={vaultLocked === false ? s : 'destrave o cofre pra ver o nome'}
                    >
                      {vaultLocked !== false && <Lock className="h-2.5 w-2.5" />}
                      {maskIfLocked(s)}
                    </span>
                  ))}
                  {cfg.secrets.length > 3 && (
                    <span className="text-xs text-slate-400">+{cfg.secrets.length - 3}</span>
                  )}
                </div>
              ) : (
                <em className="text-slate-400">nenhum</em>
              )
            }
            sub={cfg.secrets?.length ? `${cfg.secrets.length} secret${cfg.secrets.length === 1 ? '' : 's'} injetado${cfg.secrets.length === 1 ? '' : 's'} no spawn` : undefined}
            onEdit={() => setEditVault(true)}
          />
          <ConfigRow
            icon={scopeRiskIcon(cfg)}
            label="escopo"
            value={<ScopeBadge cfg={cfg} />}
            sub={scopeSummary(cfg)}
            onEdit={() => setEditScope(true)}
          />
          <ConfigRow
            icon={<Sliders className="h-3.5 w-3.5" />}
            label="autonomia"
            value={
              cfg.autonomy ? (
                <span className="font-medium uppercase text-slate-700">{cfg.autonomy.level}</span>
              ) : (
                <em className="text-slate-400">default</em>
              )
            }
            onEdit={() => setEditAuto(true)}
            sub={
              cfg.autonomy?.call_human_when && cfg.autonomy.call_human_when.length > 0
                ? `${cfg.autonomy.call_human_when.length} gatilho${
                    cfg.autonomy.call_human_when.length === 1 ? '' : 's'
                  } pra humano`
                : undefined
            }
          />
          <ConfigRow
            icon={<FileText className="h-3.5 w-3.5" />}
            label="mini-contexto"
            value={
              context ? (
                <span className="font-mono text-xs text-slate-700">
                  {context.length >= 1000 ? `${(context.length / 1000).toFixed(1)}k` : context.length} chars
                </span>
              ) : (
                <em className="text-slate-400">vazio</em>
              )
            }
            onEdit={() => setEditContext(true)}
          />
        </div>
      </div>

      {(outgoing.length > 0 || incoming.length > 0) && (
        <div className="overflow-hidden rounded-xl border bg-white shadow-sm">
          <SectionHeader icon={<ArrowRight className="h-4 w-4" />} label="Conexões" />
          <div className="space-y-3 p-3">
            {outgoing.length > 0 && (
              <div>
                <div className="mb-1.5 flex items-center gap-1 text-xs font-semibold uppercase tracking-wide text-orange-600">
                  <ArrowRight className="h-3 w-3" /> fala com
                </div>
                <div className="flex flex-wrap gap-1.5">
                  {outgoing.map((c) => (
                    <button
                      key={c}
                      onClick={() => onSelect?.(c)}
                      className="rounded-md border border-orange-200 bg-orange-50 px-2 py-1 text-xs font-medium text-orange-700 transition-colors hover:bg-orange-100"
                    >
                      {c}
                    </button>
                  ))}
                </div>
              </div>
            )}
            {incoming.length > 0 && (
              <div>
                <div className="mb-1.5 flex items-center gap-1 text-xs font-semibold uppercase tracking-wide text-blue-600">
                  <ArrowLeft className="h-3 w-3" /> recebe de
                </div>
                <div className="flex flex-wrap gap-1.5">
                  {incoming.map((c) => (
                    <button
                      key={c}
                      onClick={() => onSelect?.(c)}
                      className="rounded-md border border-blue-200 bg-blue-50 px-2 py-1 text-xs font-medium text-blue-700 transition-colors hover:bg-blue-100"
                    >
                      {c}
                    </button>
                  ))}
                </div>
              </div>
            )}
          </div>
        </div>
      )}

      {cfg.watch && (
        <div className="overflow-hidden rounded-xl border border-red-200 bg-red-50/60 shadow-sm">
          <div className="p-3">
            <div className="mb-1 flex items-center gap-1.5 text-xs font-semibold uppercase tracking-wide text-red-700">
              <Eye className="h-3.5 w-3.5" /> watch ativo
            </div>
            <div className="truncate rounded-md border border-red-200 bg-white px-2 py-1.5 font-mono text-xs text-red-700">
              {cfg.watch.query}
            </div>
            <p className="mt-1 text-xs text-red-500">
              threshold {cfg.watch.threshold}/{cfg.watch.window_minutes}min
            </p>
          </div>
        </div>
      )}

      <div className="overflow-hidden rounded-xl border bg-white shadow-sm">
        <button
          onClick={() => {
            const next = !activityOpen;
            setActivityOpen(next);
            if (next) {
              if (activityTab === 'log') loadLog();
              else loadWork();
            }
          }}
          className="flex w-full items-center gap-2 px-3 py-2.5 text-left transition-colors hover:bg-slate-50/70"
        >
          <Activity className="h-4 w-4 text-muted-foreground" />
          <h4 className="flex-1 text-xs font-semibold uppercase tracking-wide">Atividade</h4>
          {logSummary && (
            <span className="font-mono text-xs text-muted-foreground">
              {logSummary.requests} req
            </span>
          )}
          <ChevronDown
            className={cn(
              'h-3.5 w-3.5 text-muted-foreground transition-transform',
              !activityOpen && '-rotate-90',
            )}
          />
        </button>

        {logSummary?.lastErrorMsg && (
          <div className="mx-3 mb-2 flex items-start gap-1.5 rounded-md border border-red-200 bg-red-50 px-2 py-1.5 text-red-700">
            <AlertCircle className="mt-0.5 h-3 w-3 shrink-0" />
            <div className="min-w-0 flex-1">
              <div className="text-xs text-red-500">
                último erro{' '}
                {logSummary.lastErrorAt ? fmtRelativeDate(logSummary.lastErrorAt) : ''}
              </div>
              <div className="truncate font-mono text-xs">{logSummary.lastErrorMsg}</div>
            </div>
          </div>
        )}

        {activityOpen && (
          <div className="border-t">
            <div className="flex gap-1 border-b bg-slate-50/60 p-1">
              <TabButton
                active={activityTab === 'log'}
                onClick={() => {
                  setActivityTab('log');
                  loadLog();
                }}
                icon={<Activity className="h-3 w-3" />}
                label="log"
                count={logSummary?.total}
              />
              <TabButton
                active={activityTab === 'work'}
                onClick={() => {
                  setActivityTab('work');
                  loadWork();
                }}
                icon={<Hammer className="h-3 w-3" />}
                label="trabalho"
              />
            </div>
            <div className="soft-scroll max-h-64 overflow-y-auto p-2">
              {activityTab === 'log' ? (
                <>
                  {logLoading && <EmptyState text="carregando…" />}
                  {!logLoading && logEntries && logEntries.length === 0 && (
                    <EmptyState text="sem eventos ainda" />
                  )}
                  {!logLoading &&
                    logEntries &&
                    [...logEntries].reverse().map((e, i) => <LogRow key={i} e={e} />)}
                </>
              ) : (
                <>
                  {workLoading && <EmptyState text="carregando…" />}
                  {!workLoading && workEntries && workEntries.length === 0 && (
                    <EmptyState text="nenhuma edição/comando atribuído ainda" />
                  )}
                  {!workLoading && workEntries && workEntries.map((e, i) => <WorkRow key={i} e={e} />)}
                </>
              )}
            </div>
          </div>
        )}
      </div>

      {!isProtected && (
        <div>
          <button
            type="button"
            onClick={handleDelete}
            disabled={deleting}
            className="flex w-full items-center justify-center gap-1.5 rounded-lg border border-red-200 bg-white px-3 py-2 text-xs font-medium text-red-700 transition-colors hover:border-red-300 hover:bg-red-50 disabled:opacity-50"
            title="remover agente"
          >
            <Trash2 className="h-3.5 w-3.5" />
            {deleting ? 'removendo…' : 'remover agente'}
          </button>
          {deleteErr && (
            <p className="mt-1.5 rounded-md border border-red-200 bg-red-50 px-2 py-1.5 text-xs text-red-700">
              {deleteErr}
            </p>
          )}
        </div>
      )}

      {editAuto && (
        <AutonomyEditor
          name={name}
          cfg={cfg}
          origin={manifest?._meta?.agent_origin?.[name] || null}
          onClose={() => setEditAuto(false)}
        />
      )}
      {editModel && (
        <ModelEditor
          name={name}
          current={cfg.model || ''}
          origin={manifest?._meta?.agent_origin?.[name] || null}
          onClose={() => setEditModel(false)}
        />
      )}
      {editContext && (
        <ContextEditor name={name} current={context} onClose={() => setEditContext(false)} />
      )}
      {editMcps && (
        <McpsEditor name={name} current={cfg.mcps || []} onClose={() => setEditMcps(false)} />
      )}
      {editScope && (
        <ScopeEditor
          name={name}
          currentTools={cfg.tools || []}
          currentBashAllow={cfg.scope?.bash_allow || []}
          onClose={() => setEditScope(false)}
        />
      )}
      {editVault && (
        <VaultAccessEditor
          name={name}
          current={cfg.secrets || []}
          onClose={() => setEditVault(false)}
        />
      )}
    </div>
  );
}

// ── Escopo (sandbox) ──────────────────────────────────────────────────────────

const AVAILABLE_TOOLS = [
  'Read', 'Grep', 'Glob', 'Edit', 'Write', 'Bash',
  'WebFetch', 'WebSearch', 'Task', 'TodoWrite', 'NotebookEdit',
] as const;

type RiskLevel = 'readonly' | 'write-local' | 'bash-restrito' | 'bash-livre';

function classifyRisk(cfg: AgentCfg): RiskLevel {
  const tools = cfg.tools || [];
  const hasBash = tools.includes('Bash');
  const hasWrite = tools.includes('Write') || tools.includes('Edit');
  const bashAllow = cfg.scope?.bash_allow || [];
  if (!hasBash && !hasWrite) return 'readonly';
  if (!hasBash && hasWrite) return 'write-local';
  if (hasBash && bashAllow.length > 0) return 'bash-restrito';
  return 'bash-livre';
}

function riskColor(level: RiskLevel): string {
  return {
    'readonly':      'border-green-200 bg-green-50 text-green-700',
    'write-local':   'border-yellow-200 bg-yellow-50 text-yellow-700',
    'bash-restrito': 'border-orange-200 bg-orange-50 text-orange-700',
    'bash-livre':    'border-red-300 bg-red-50 text-red-700',
  }[level];
}

function riskLabel(level: RiskLevel): string {
  return {
    'readonly':      'read-only',
    'write-local':   'write local',
    'bash-restrito': 'bash restrito',
    'bash-livre':    'bash desligado',
  }[level];
}

function scopeRiskIcon(cfg: AgentCfg) {
  const level = classifyRisk(cfg);
  if (level === 'readonly') return <ShieldCheck className="h-3.5 w-3.5 text-green-600" />;
  if (level === 'bash-livre') return <ShieldAlert className="h-3.5 w-3.5 text-red-600" />;
  return <Shield className="h-3.5 w-3.5 text-orange-600" />;
}

function ScopeBadge({ cfg }: { cfg: AgentCfg }) {
  const level = classifyRisk(cfg);
  return (
    <span className={cn('rounded-md border px-1.5 py-0 text-xs font-medium', riskColor(level))}>
      {riskLabel(level)}
    </span>
  );
}

function scopeSummary(cfg: AgentCfg): string | undefined {
  const tools = cfg.tools || [];
  const bashAllow = cfg.scope?.bash_allow || [];
  const parts: string[] = [];
  if (tools.length) parts.push(`${tools.length} tool${tools.length === 1 ? '' : 's'}`);
  if (bashAllow.length) parts.push(`${bashAllow.length} bash pattern${bashAllow.length === 1 ? '' : 's'}`);
  return parts.length ? parts.join(' · ') : 'sem escopo definido';
}

interface ScopeEditorProps {
  name: string;
  currentTools: string[];
  currentBashAllow: string[];
  onClose: () => void;
}

function ScopeEditor({ name, currentTools, currentBashAllow, onClose }: ScopeEditorProps) {
  const [tools, setTools] = useState<string[]>([...currentTools]);
  const [bashText, setBashText] = useState<string>(currentBashAllow.join('\n'));
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const bashAllow = bashText
    .split('\n')
    .map((s) => s.trim())
    .filter((s) => s.length > 0);

  const toggleTool = (t: string) => {
    setTools((cur) => (cur.includes(t) ? cur.filter((x) => x !== t) : [...cur, t]));
  };

  const level = classifyRisk({ tools, scope: { bash_allow: bashAllow } } as AgentCfg);

  const preview = buildAllowedToolsPreview(tools, bashAllow);

  async function save() {
    setSaving(true);
    setErr(null);
    try {
      await updateAgentScope(name, {
        tools,
        scope: { bash_allow: bashAllow },
      });
      onClose();
      window.location.reload();
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4">
      <div className="max-h-[90vh] w-full max-w-2xl overflow-auto rounded-xl bg-white shadow-xl">
        <div className="flex items-center justify-between border-b px-4 py-3">
          <div className="flex items-center gap-2">
            <Shield className="h-4 w-4 text-slate-600" />
            <h3 className="text-sm font-semibold">Escopo · {name}</h3>
            <span className={cn('rounded-md border px-1.5 py-0 text-xs font-medium', riskColor(level))}>
              {riskLabel(level)}
            </span>
          </div>
          <button onClick={onClose} className="text-slate-400 hover:text-slate-600">
            <X className="h-4 w-4" />
          </button>
        </div>

        <div className="space-y-5 p-4">
          <section>
            <h4 className="mb-2 text-xs font-semibold uppercase tracking-wide text-slate-600">
              Ferramentas
            </h4>
            <p className="mb-2 text-xs text-slate-500">
              Marque só o que o agente precisa. Sem <code>Read/Grep/Glob</code> ele não lê arquivos.
              Sem <code>Write/Edit</code> não altera nada. <code>Bash</code> sem padrões abaixo fica desativado.
            </p>
            <div className="grid grid-cols-3 gap-1.5">
              {AVAILABLE_TOOLS.map((t) => (
                <label
                  key={t}
                  className={cn(
                    'flex cursor-pointer items-center gap-1.5 rounded-md border px-2 py-1 text-xs',
                    tools.includes(t)
                      ? 'border-blue-300 bg-blue-50 text-blue-800'
                      : 'border-slate-200 bg-white text-slate-700 hover:bg-slate-50',
                  )}
                >
                  <input
                    type="checkbox"
                    checked={tools.includes(t)}
                    onChange={() => toggleTool(t)}
                    className="h-3 w-3"
                  />
                  <span className="font-mono">{t}</span>
                </label>
              ))}
            </div>
          </section>

          <section>
            <h4 className="mb-2 text-xs font-semibold uppercase tracking-wide text-slate-600">
              Bash · comandos permitidos
            </h4>
            <p className="mb-2 text-xs text-slate-500">
              Um padrão por linha. Ex: <code>git *</code>, <code>npm run *</code>, <code>curl http://localhost:37778/*</code>.
              Padrões proibidos globalmente (<code>rm</code>, <code>sudo</code>, <code>chmod</code>, etc)
              são bloqueados mesmo se você incluir aqui.
            </p>
            <textarea
              value={bashText}
              onChange={(e) => setBashText(e.target.value)}
              placeholder={'git *\nnpm run *'}
              className="w-full rounded-md border border-slate-200 bg-slate-50 px-2 py-1.5 font-mono text-xs"
              rows={6}
            />
          </section>

          <section>
            <h4 className="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-600">
              Preview do <code>--allowed-tools</code>
            </h4>
            <pre className="overflow-auto rounded-md bg-slate-900 px-2 py-1.5 font-mono text-xs text-green-300">
              {preview || '<vazio — agente não poderá fazer nada>'}
            </pre>
          </section>

          {err && (
            <div className="rounded-md border border-red-200 bg-red-50 px-2 py-1 text-xs text-red-700">
              {err}
            </div>
          )}

          <div className="flex justify-end gap-2 border-t pt-3">
            <button
              onClick={onClose}
              className="rounded-md border px-3 py-1 text-xs hover:bg-slate-50"
            >
              Cancelar
            </button>
            <button
              onClick={save}
              disabled={saving}
              className="inline-flex items-center gap-1 rounded-md bg-blue-600 px-3 py-1 text-xs font-medium text-white hover:bg-blue-700 disabled:opacity-50"
            >
              <Save className="h-3.5 w-3.5" />
              {saving ? 'Salvando…' : 'Salvar'}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

function buildAllowedToolsPreview(tools: string[], bashAllow: string[]): string {
  const out: string[] = [];
  for (const t of tools) {
    if (t === 'Bash') {
      if (!bashAllow.length) continue;
      for (const p of bashAllow) out.push(`Bash(${p})`);
    } else {
      out.push(t);
    }
  }
  return out.join(',');
}

function SectionHeader({ icon, label }: { icon: React.ReactNode; label: string }) {
  return (
    <div className="flex items-center gap-2 border-b bg-slate-50/60 px-3 py-2">
      <span className="text-muted-foreground">{icon}</span>
      <h4 className="text-xs font-semibold uppercase tracking-wide">{label}</h4>
    </div>
  );
}

function MetricCell({
  icon,
  label,
  value,
  tone = 'default',
  small = false,
}: {
  icon: React.ReactNode;
  label: string;
  value: string | number;
  tone?: 'default' | 'danger';
  small?: boolean;
}) {
  return (
    <div className="flex flex-col items-center justify-center gap-0.5 px-2 py-2.5">
      <span
        className={cn(
          'tabular-nums leading-none',
          small ? 'text-xs font-medium' : 'text-base font-semibold',
          tone === 'danger' && value !== 0 && 'text-red-600',
        )}
      >
        {value}
      </span>
      <span className="flex items-center gap-0.5 text-xs uppercase tracking-wide text-muted-foreground">
        {icon} {label}
      </span>
    </div>
  );
}

function ConfigRow({
  icon,
  label,
  value,
  sub,
  onEdit,
}: {
  icon: React.ReactNode;
  label: string;
  value: React.ReactNode;
  sub?: string;
  onEdit: () => void;
}) {
  return (
    <div className="group flex items-center gap-2 px-3 py-2 transition-colors hover:bg-slate-50/70">
      <span className="text-muted-foreground">{icon}</span>
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="text-xs uppercase tracking-wide text-muted-foreground">{label}</span>
        {sub && <span className="text-xs text-slate-500">{sub}</span>}
      </div>
      <div className="flex min-w-0 shrink-0 items-center gap-1.5 text-xs">
        <div className="max-w-[150px] truncate text-right">{value}</div>
        <button
          onClick={onEdit}
          className="rounded-md border border-transparent bg-slate-100 p-1 text-slate-500 opacity-0 transition-opacity hover:border-indigo-200 hover:bg-indigo-50 hover:text-indigo-700 group-hover:opacity-100"
          title={`editar ${label}`}
        >
          <Pencil className="h-3 w-3" />
        </button>
      </div>
    </div>
  );
}

function TabButton({
  active,
  onClick,
  icon,
  label,
  count,
}: {
  active: boolean;
  onClick: () => void;
  icon: React.ReactNode;
  label: string;
  count?: number;
}) {
  return (
    <button
      onClick={onClick}
      className={cn(
        'flex flex-1 items-center justify-center gap-1.5 rounded-md px-2 py-1 text-xs font-medium transition-colors',
        active
          ? 'bg-white text-slate-800 shadow-sm'
          : 'text-muted-foreground hover:bg-white/60 hover:text-slate-700',
      )}
    >
      {icon} {label}
      {count != null && count > 0 && (
        <span className="rounded-full bg-slate-200 px-1 font-mono text-xs text-slate-600">
          {count}
        </span>
      )}
    </button>
  );
}

function EmptyState({ text }: { text: string }) {
  return <div className="py-4 text-center text-xs text-slate-400">{text}</div>;
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
    <div className="flex items-start gap-1.5 py-0.5 text-xs leading-tight">
      <span className="shrink-0 text-slate-400">{e.ts.slice(11, 19)}</span>
      <span className={`shrink-0 rounded px-1 font-semibold uppercase ${color}`}>{e.type}</span>
      <span className="min-w-0 flex-1 truncate text-slate-700">{detail}</span>
    </div>
  );
}

function WorkRow({ e }: { e: WorkEntry }) {
  if (e.type === 'file_edit') {
    const ins = e.diff?.ins;
    const del = e.diff?.del;
    return (
      <div className="flex items-start gap-1.5 py-0.5 text-xs leading-tight">
        <span className="shrink-0 text-slate-400">{e.ts.slice(11, 19)}</span>
        <FilePen className="mt-0.5 h-3 w-3 shrink-0 text-emerald-600" />
        <span className="min-w-0 flex-1 truncate font-mono text-slate-700" title={e.file}>
          {e.file}
        </span>
        {(typeof ins === 'number' || typeof del === 'number') && (
          <span className="shrink-0 text-xs">
            {typeof ins === 'number' && <span className="text-emerald-700">+{ins}</span>}
            {typeof del === 'number' && <span className="ml-0.5 text-red-600">-{del}</span>}
          </span>
        )}
      </div>
    );
  }
  if (e.type === 'bash') {
    return (
      <div className="flex items-start gap-1.5 py-0.5 text-xs leading-tight">
        <span className="shrink-0 text-slate-400">{e.ts.slice(11, 19)}</span>
        <Terminal className="mt-0.5 h-3 w-3 shrink-0 text-slate-600" />
        <span className="min-w-0 flex-1 truncate font-mono text-slate-700" title={e.cmd}>
          {e.cmd}
        </span>
        {e.output_summary && (
          <span className="shrink-0 text-xs text-slate-500" title={e.output_summary}>
            {e.output_summary.slice(0, 24)}
          </span>
        )}
      </div>
    );
  }
  return (
    <div className="flex items-start gap-1.5 py-0.5 text-xs leading-tight">
      <span className="shrink-0 text-slate-400">{e.ts.slice(11, 19)}</span>
      <ArrowRight className="mt-0.5 h-3 w-3 shrink-0 text-indigo-600" />
      <span className="min-w-0 flex-1 truncate text-slate-700" title={e.prompt}>
        → {e.agent}: {e.description}
      </span>
    </div>
  );
}

function AutonomyBadge({ level }: { level: AutonomyLevel }) {
  const styles: Record<AutonomyLevel, string> = {
    low: 'border-amber-200 bg-amber-50 text-amber-700',
    medium: 'border-slate-200 bg-slate-50 text-slate-600',
    high: 'border-emerald-200 bg-emerald-50 text-emerald-700',
  };
  return (
    <span
      title={`autonomia ${level}`}
      className={`rounded-full border px-1.5 py-0 text-xs font-semibold uppercase tracking-wide ${styles[level]}`}
    >
      {level}
    </span>
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
            <p className="text-xs text-muted-foreground">
              quando o agente decide sozinho e quando chama você
            </p>
          </div>
          <Button size="icon" variant="ghost" className="h-7 w-7" onClick={onClose}>
            <X className="h-4 w-4" />
          </Button>
        </header>
        <div className="soft-scroll flex-1 overflow-y-auto p-4 text-xs">
          <label className="mb-1 block text-xs font-semibold uppercase text-slate-600">
            Nível
          </label>
          <div className="mb-3 grid grid-cols-3 gap-1.5">
            {(['low', 'medium', 'high'] as AutonomyLevel[]).map((lv) => (
              <button
                key={lv}
                onClick={() => setLevel(lv)}
                className={`rounded border px-2 py-1.5 text-xs font-medium transition ${
                  level === lv
                    ? 'border-indigo-500 bg-indigo-50 text-indigo-700'
                    : 'border-slate-200 hover:bg-slate-50'
                }`}
              >
                {lv}
              </button>
            ))}
          </div>
          <p className="mb-3 rounded bg-slate-50 px-2 py-1.5 text-xs text-slate-600">
            {LEVEL_DESC[level]}
          </p>

          <label className="mb-1 block text-xs font-semibold uppercase text-slate-600">
            Sempre chamar o humano quando… <span className="text-slate-400">(uma por linha)</span>
          </label>
          <textarea
            value={cases}
            onChange={(e) => setCases(e.target.value)}
            rows={6}
            className="mb-3 w-full rounded border border-slate-200 bg-white px-2 py-1.5 font-mono text-xs focus:border-indigo-400 focus:outline-none"
            placeholder="decisão envolve gastar dinheiro&#10;mudança vai pra produção&#10;dois especialistas discordam"
          />

          <label className="mb-3 flex items-start gap-2 text-xs">
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
              <span className="text-xs text-slate-500">
                se marcado, a mesma mudança é gravada no template — próximos projetos já vêm com essa autonomia.
              </span>
            </span>
          </label>

          {err && (
            <div className="mb-2 rounded border border-red-200 bg-red-50 px-2 py-1.5 text-xs text-red-700">
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

const MODEL_SUGGESTIONS = [
  'opus',
  'sonnet',
  'haiku',
  'opus-5.5',
  'sonnet-5.5',
  'haiku-5.5',
  'opus-4.7',
  'opus-4.6',
  'sonnet-4.6',
  'sonnet-4.5',
  'haiku-4.5',
  'claude-opus-5-5',
  'claude-opus-4-7',
  'claude-sonnet-4-6',
  'claude-haiku-4-5-20251001',
];

interface ModelEditorProps {
  name: string;
  current: string;
  origin: { category: string; slug: string } | null;
  onClose: () => void;
}

function ModelEditor({ name, current, origin, onClose }: ModelEditorProps) {
  const [model, setModel] = useState<string>(current || '');
  const [propagate, setPropagate] = useState<boolean>(!!origin);
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const save = async () => {
    setErr(null);
    setSaving(true);
    try {
      await updateModel(name, {
        model: model.trim(),
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
              <Cpu className="h-4 w-4" /> Modelo de {name}
            </div>
            <p className="text-xs text-muted-foreground">
              versão do modelo que o agente usa ao responder
            </p>
          </div>
          <Button size="icon" variant="ghost" className="h-7 w-7" onClick={onClose}>
            <X className="h-4 w-4" />
          </Button>
        </header>
        <div className="soft-scroll flex-1 overflow-y-auto p-4 text-xs">
          <label className="mb-1 block text-xs font-semibold uppercase text-slate-600">
            Modelo <span className="text-slate-400">(deixe vazio pra usar o default)</span>
          </label>
          <input
            list="model-suggestions"
            value={model}
            onChange={(e) => setModel(e.target.value)}
            placeholder="opus, sonnet, opus-4.7, claude-sonnet-4-6…"
            className="mb-2 w-full rounded border border-slate-200 bg-white px-2 py-1.5 font-mono text-xs focus:border-indigo-400 focus:outline-none"
          />
          <datalist id="model-suggestions">
            {MODEL_SUGGESTIONS.map((m) => (
              <option key={m} value={m} />
            ))}
          </datalist>
          <p className="mb-3 text-xs text-slate-500">
            aceita aliases curtos (opus/sonnet/haiku) ou IDs completos (claude-opus-4-7). o valor
            é passado direto ao <code className="rounded bg-slate-100 px-1">claude -p --model</code>.
          </p>

          <label className="mb-3 flex items-start gap-2 text-xs">
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
              <span className="text-xs text-slate-500">
                se marcado, a mesma mudança é gravada no template — próximos projetos já vêm com esse modelo.
              </span>
            </span>
          </label>

          {err && (
            <div className="mb-2 rounded border border-red-200 bg-red-50 px-2 py-1.5 text-xs text-red-700">
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

interface ContextEditorProps {
  name: string;
  current: string;
  onClose: () => void;
}

function ContextEditor({ name, current, onClose }: ContextEditorProps) {
  const [content, setContent] = useState<string>(current || '');
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const dirty = content !== (current || '');

  const save = async () => {
    setErr(null);
    setSaving(true);
    try {
      await updateAgentContext(name, content);
      onClose();
    } catch (e: any) {
      setErr(e?.message || 'falha ao salvar');
    } finally {
      setSaving(false);
    }
  };

  const tryClose = () => {
    if (saving) return;
    if (dirty && !confirm('Descartar alterações do mini-contexto?')) return;
    onClose();
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/50 backdrop-blur-sm p-4">
      <div className="flex max-h-[90vh] w-full max-w-3xl flex-col overflow-hidden rounded-xl border bg-white shadow-2xl">
        <header className="flex items-center justify-between border-b px-4 py-3">
          <div>
            <div className="flex items-center gap-2 text-sm font-semibold">
              <FileText className="h-4 w-4" /> mini-contexto de {name}
            </div>
            <p className="text-xs text-muted-foreground">
              markdown injetado no prompt do agente a cada conversa
            </p>
          </div>
          <Button size="icon" variant="ghost" className="h-7 w-7" onClick={tryClose}>
            <X className="h-4 w-4" />
          </Button>
        </header>
        <div className="flex-1 overflow-hidden p-4">
          <textarea
            value={content}
            onChange={(e) => setContent(e.target.value)}
            spellCheck={false}
            className="h-[60vh] w-full resize-none rounded border border-slate-200 bg-white p-3 font-mono text-xs leading-snug focus:border-indigo-400 focus:outline-none"
            placeholder={'# Responsabilidade\n...\n\n## Princípios\n...'}
          />
          <div className="mt-2 flex items-center justify-between text-xs text-slate-500">
            <span>{content.length} chars · markdown</span>
            {dirty && <span className="text-amber-600">alterações não salvas</span>}
          </div>
          {err && (
            <div className="mt-2 rounded border border-red-200 bg-red-50 px-2 py-1.5 text-xs text-red-700">
              {err}
            </div>
          )}
        </div>
        <footer className="flex justify-end gap-2 border-t px-4 py-3">
          <Button size="sm" variant="outline" onClick={tryClose} disabled={saving}>
            cancelar
          </Button>
          <Button size="sm" onClick={save} disabled={saving || !dirty} className="gap-1.5">
            <Save className="h-3.5 w-3.5" />
            {saving ? 'salvando…' : 'salvar'}
          </Button>
        </footer>
      </div>
    </div>
  );
}

interface McpsEditorProps {
  name: string;
  current: string[];
  onClose: () => void;
}

function McpsEditor({ name, current, onClose }: McpsEditorProps) {
  const [draft, setDraft] = useState<string>((current || []).join(', '));
  const [registered, setRegistered] = useState<RegisteredMcp[]>([]);
  const [others, setOthers] = useState<string[]>([]);
  // nomes que têm conexão REAL: registrados (→ .mcp.json) ou definidos no ~/.claude.json.
  const [connectable, setConnectable] = useState<Set<string>>(new Set());
  const [loaded, setLoaded] = useState(false);
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  // sub-modal de cadastro/edição da DEF do servidor. null = fechado;
  // { name: null } = novo; { name: 'x' } = editar.
  const [mcpForm, setMcpForm] = useState<{ name: string | null } | null>(null);
  const isMobile = useMediaQuery('(max-width: 768px)');

  const reload = () => {
    listMcps()
      .then((r) => {
        const reg = r.registered || [];
        setRegistered(reg);
        const regNames = reg.map((m) => m.name);
        // sugestões "soltas" = tudo que aparece no catálogo mas não está no registry
        setOthers((r.all || []).filter((n) => !regNames.includes(n)));
        // conexão real = registrado OU presente no ~/.claude.json (global/projeto).
        // "referenced" (citado por outro agente) NÃO conta — pode ser órfão também.
        setConnectable(new Set([...regNames, ...(r.global || []), ...(r.project || [])]));
        setLoaded(true);
      })
      .catch(() => setLoaded(true));
  };
  useEffect(reload, []);

  const splitList = (v: string) => v.split(/[,\n]/).map((s) => s.trim()).filter(Boolean);
  const parsed = splitList(draft);
  const has = (n: string) => parsed.includes(n);
  // atribuídos sem definição de servidor → entram no allowlist mas não conectam.
  const orphans = loaded ? parsed.filter((n) => !connectable.has(n)) : [];

  const toggle = (n: string) => {
    const cur = splitList(draft);
    const next = cur.includes(n) ? cur.filter((x) => x !== n) : [...cur, n];
    setDraft(next.join(', '));
  };
  const add = (n: string) => {
    const cur = splitList(draft);
    if (!cur.includes(n)) setDraft([...cur, n].join(', '));
  };

  const save = async () => {
    setErr(null);
    setSaving(true);
    try {
      await updateAgentMcps(name, parsed);
      onClose();
    } catch (e: any) {
      setErr(e?.message || 'falha ao salvar');
    } finally {
      setSaving(false);
    }
  };

  return (
    <>
    <Modal
      opened
      onClose={onClose}
      fullScreen={isMobile}
      size="md"
      title={
        <div>
          <div className="flex items-center gap-2 text-sm font-semibold">
            <Plug className="h-4 w-4" /> MCPs de {name}
          </div>
          <Text size="xs" muted>servidores MCP que o agente pode acessar quando invocado</Text>
        </div>
      }
    >
      <Stack gap="sm">
        <div>
          <label className="mb-1 block text-xs font-semibold uppercase text-slate-600">
            MCPs <span className="font-normal normal-case text-slate-400">(separe por vírgula ou quebra de linha)</span>
          </label>
          <Textarea
            value={draft}
            onChange={setDraft}
            placeholder="ex: resend, supabase"
            minRows={2}
          />
        </div>

        <div>
          <div className="mb-1 flex items-center justify-between gap-2">
            <Text size="xs" muted>registrados no projeto (clique p/ adicionar, lápis p/ editar a def)</Text>
            <UiButton size="compact-xs" variant="light" leftIcon={<Plus className="h-3 w-3" />} onClick={() => setMcpForm({ name: null })}>
              novo
            </UiButton>
          </div>
          {registered.length === 0 ? (
            <Text size="xs" muted>Nenhum MCP registrado. Use "novo" pra criar a conexão real.</Text>
          ) : (
            <Group gap="xs">
              {registered.map((m) => (
                <span key={m.name} className="inline-flex items-center gap-0.5 rounded-full bg-slate-50 pr-0.5">
                  <Chip checked={has(m.name)} onChange={() => toggle(m.name)} variant="light">
                    <span className="inline-flex items-center gap-1">
                      {m.name}
                      <UiBadge color={m.storage === 'vault' ? 'warning' : 'info'} variant="light" size="sm">
                        {m.storage === 'vault' ? 'cofre' : 'banco'}
                      </UiBadge>
                    </span>
                  </Chip>
                  <ActionIcon size="sm" variant="subtle" color="gray" label={`editar ${m.name}`} onClick={() => setMcpForm({ name: m.name })}>
                    <Pencil className="h-3.5 w-3.5" />
                  </ActionIcon>
                </span>
              ))}
            </Group>
          )}
        </div>

        {others.length > 0 && (
          <div>
            <Text size="xs" muted style={{ marginBottom: 4 }}>de ~/.claude.json e outros agentes</Text>
            <Group gap="xs">
              {others.map((n) => (
                <Chip key={n} checked={has(n)} onChange={() => toggle(n)} variant="light" color="gray">{n}</Chip>
              ))}
            </Group>
          </div>
        )}

        <Text size="xs" muted>
          MCPs <strong>registrados</strong> são materializados no <code>.mcp.json</code> do projeto
          (conexão real) e a lista por-agente vira o allowlist <code>--allowed-tools</code> no spawn —
          então o agente só acessa os que estiverem aqui. Pode digitar um nome novo — basta confirmar.
        </Text>

        {orphans.length > 0 && (
          <div className="rounded border border-amber-200 bg-amber-50 px-2 py-1.5">
            <Text size="xs" style={{ color: '#92400e' }}>
              <strong>{orphans.join(', ')}</strong> {orphans.length > 1 ? 'não têm' : 'não tem'} conexão real:{' '}
              {orphans.length > 1 ? 'não estão' : 'não está'} no registry nem no <code>~/.claude.json</code>.
              {' '}Vão pro allowlist do agente, mas não conectam até serem registrados em "Servidores MCP".
            </Text>
          </div>
        )}

        {err && <Text size="xs" color="danger">{err}</Text>}

        <Group justify="flex-end" gap="xs">
          <UiButton size="sm" variant="default" color="gray" onClick={onClose} disabled={saving}>
            cancelar
          </UiButton>
          <UiButton size="sm" onClick={save} loading={saving} leftIcon={<Save className="h-3.5 w-3.5" />}>
            {saving ? 'salvando…' : 'salvar'}
          </UiButton>
        </Group>
      </Stack>
    </Modal>

    {mcpForm && (
      <Modal
        opened
        onClose={() => setMcpForm(null)}
        fullScreen={isMobile}
        size="md"
        title={<Text fw={600}>{mcpForm.name ? `Editar MCP: ${mcpForm.name}` : 'Registrar MCP'}</Text>}
      >
        <McpForm
          editName={mcpForm.name}
          onCancel={() => setMcpForm(null)}
          onSaved={(savedName) => {
            // registrou/editou a def → recarrega catálogo e já atribui ao agente.
            setMcpForm(null);
            add(savedName);
            reload();
          }}
        />
      </Modal>
    )}
    </>
  );
}

// ── Editor do escopo do cofre (secrets injetados como env vars) ──────────────

interface VaultAccessEditorProps {
  name: string;
  current: string[];
  onClose: () => void;
}

function VaultAccessEditor({ name, current, onClose }: VaultAccessEditorProps) {
  const [available, setAvailable] = useState<string[] | null>(null);
  const [selected, setSelected] = useState<Set<string>>(() => new Set(current));
  const [vaultStatus, setVaultStatus] = useState<{ initialized: boolean; locked: boolean } | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const st = await getVaultStatus();
        if (!alive) return;
        setVaultStatus({ initialized: st.initialized, locked: st.locked });
        if (st.initialized && !st.locked) {
          const keys = await listVaultKeys();
          if (alive) setAvailable(keys);
        } else {
          if (alive) setAvailable([]);
        }
      } catch (e: any) {
        if (alive) setErr(e?.message || 'falha ao ler cofre');
      } finally {
        if (alive) setLoading(false);
      }
    })();
    return () => { alive = false; };
  }, []);

  const toggle = (key: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  const save = async () => {
    setErr(null);
    setSaving(true);
    try {
      await updateAgentVault(name, Array.from(selected).sort());
      onClose();
    } catch (e: any) {
      setErr(e?.message || 'falha ao salvar');
    } finally {
      setSaving(false);
    }
  };

  const missingInVault = Array.from(selected).filter((k) => available && !available.includes(k));
  const allKeys = Array.from(new Set([...(available || []), ...current])).sort();

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/50 backdrop-blur-sm p-4">
      <div className="flex max-h-[85vh] w-full max-w-md flex-col overflow-hidden rounded-xl border bg-white shadow-2xl">
        <header className="flex items-center justify-between border-b px-4 py-3">
          <div>
            <div className="flex items-center gap-2 text-sm font-semibold">
              <KeyRound className="h-4 w-4 text-amber-600" /> Cofre de {name}
            </div>
            <p className="text-sm text-muted-foreground">
              secrets injetados como env vars no spawn do agente
            </p>
          </div>
          <Button size="icon" variant="ghost" className="h-7 w-7" onClick={onClose}>
            <X className="h-4 w-4" />
          </Button>
        </header>

        <div className="soft-scroll flex-1 overflow-y-auto p-4 text-sm space-y-3">
          {loading && <p className="text-slate-500">carregando secrets…</p>}

          {!loading && vaultStatus && !vaultStatus.initialized && (
            <div className="rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-sm text-amber-800">
              <div className="font-semibold">Cofre não inicializado</div>
              <p className="mt-1">crie o cofre na aba "Cofre" antes de configurar secrets por agente.</p>
            </div>
          )}

          {!loading && vaultStatus?.initialized && vaultStatus.locked && (
            <div className="rounded-md border border-slate-200 bg-slate-50 px-3 py-2 text-sm text-slate-700">
              <div className="flex items-center gap-1.5 font-semibold">
                <Lock className="h-3 w-3" /> UI do cofre travada
              </div>
              <p className="mt-1">
                desbloqueie o cofre (painel "Cofre") pra ver a lista de secrets disponíveis. os secrets já vinculados
                continuam sendo injetados nos agentes mesmo com a UI travada.
              </p>
              {current.length > 0 && (
                <>
                  <p className="mt-2 font-semibold text-slate-800">
                    atualmente vinculados: <span className="font-normal text-slate-500">({current.length})</span>
                  </p>
                  <p className="mt-1 text-sm text-slate-500">nomes ocultos — destrave o cofre pra ver.</p>
                  <div className="mt-1 flex flex-wrap gap-1">
                    {current.map((_, i) => (
                      <span
                        key={i}
                        className="flex items-center gap-1 rounded border border-amber-200 bg-amber-50 px-1.5 py-0 font-mono text-sm text-amber-700"
                      >
                        <Lock className="h-2.5 w-2.5" />••••••••
                      </span>
                    ))}
                  </div>
                </>
              )}
            </div>
          )}

          {!loading && vaultStatus?.initialized && !vaultStatus.locked && (
            <>
              <div className="rounded-md border border-indigo-100 bg-indigo-50/50 px-3 py-2 text-sm text-slate-700">
                marque os secrets que este agente pode usar. eles viram env vars (ex:{' '}
                <code className="rounded bg-white px-1">ASAAS_TOKEN</code>) no subprocess do agente. fora daqui, o
                agente não enxerga o resto do cofre.
              </div>

              {allKeys.length === 0 ? (
                <p className="text-slate-500">
                  nenhum secret no cofre ainda. adicione em <strong>Cofre → novo secret</strong> primeiro.
                </p>
              ) : (
                <ul className="space-y-1">
                  {allKeys.map((k) => {
                    const checked = selected.has(k);
                    const missing = !!available && !available.includes(k);
                    return (
                      <li key={k}>
                        <label
                          className={cn(
                            'flex cursor-pointer items-center gap-2 rounded-md border px-2.5 py-1.5 transition',
                            checked ? 'border-amber-300 bg-amber-50' : 'border-slate-200 bg-white hover:bg-slate-50',
                            missing && 'border-red-200 bg-red-50/50',
                          )}
                        >
                          <input
                            type="checkbox"
                            checked={checked}
                            onChange={() => toggle(k)}
                            className="h-3.5 w-3.5"
                          />
                          <code className="flex-1 font-mono text-sm text-slate-900">{k}</code>
                          {missing && (
                            <span className="text-sm text-red-600" title="secret vinculado mas não existe no cofre">
                              ausente
                            </span>
                          )}
                        </label>
                      </li>
                    );
                  })}
                </ul>
              )}

              {missingInVault.length > 0 && (
                <div className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700">
                  <strong>{missingInVault.length} secret(s) vinculados mas não existem no cofre</strong> — o agente
                  roda sem essas envs e loga <code>vault_partial</code>. desmarque pra limpar a lista.
                </div>
              )}

              <p className="text-sm text-slate-500">
                mudanças aqui afetam o próximo spawn do agente. secrets não ficam no prompt — são env vars do processo.
              </p>
            </>
          )}

          {err && (
            <div className="rounded border border-red-200 bg-red-50 px-2 py-1.5 text-sm text-red-700">
              {err}
            </div>
          )}
        </div>

        <footer className="flex justify-end gap-2 border-t px-4 py-3">
          <Button size="sm" variant="outline" onClick={onClose} disabled={saving}>
            cancelar
          </Button>
          <Button size="sm" onClick={save} disabled={saving || loading} className="gap-1.5">
            <Save className="h-3.5 w-3.5" />
            {saving ? 'salvando…' : 'salvar'}
          </Button>
        </footer>
      </div>
    </div>
  );
}
