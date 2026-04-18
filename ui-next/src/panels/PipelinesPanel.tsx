import { useEffect, useMemo, useState } from 'react';
import { X, Workflow, Play, Check, XCircle, ChevronRight, Clock, Square, CircleDot, AlertCircle, History, Plus, Trash2, Sparkles } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import {
  listPipelines,
  installPipeline,
  getPipelineState,
  startPipeline,
  advancePipeline,
  cancelPipeline,
  clearPipeline,
  listPipelineRuns,
  createPipeline,
  type PipelineDef,
  type PipelineRunState,
  type PipelineRunSummary,
  type PipelineStatus,
  type PipelineStepDef,
  type PipelineStepType,
} from '@/lib/pipelines';

interface Props {
  onClose: () => void;
  meHumanId: string | null;
  meHumanName: string | null;
  onOpenChat?: (agentName: string) => void;
}

export function PipelinesPanel({ onClose, meHumanId, meHumanName, onOpenChat }: Props) {
  const [pipelines, setPipelines] = useState<PipelineDef[] | null>(null);
  const [state, setState] = useState<PipelineRunState | null>(null);
  const [runs, setRuns] = useState<PipelineRunSummary[]>([]);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState('');
  const [newOpen, setNewOpen] = useState(false);
  const [agentNames, setAgentNames] = useState<string[]>([]);

  const refresh = async () => {
    try {
      const [{ pipelines: list }, { state: st }, { runs: rr }] = await Promise.all([
        listPipelines(),
        getPipelineState(),
        listPipelineRuns(50),
      ]);
      setPipelines(list);
      setState(st);
      setRuns(rr);
    } catch (e: any) {
      setErr(e?.message || 'falha ao carregar');
    }
  };

  useEffect(() => {
    refresh();
    const t = setInterval(refresh, 3000);
    return () => clearInterval(t);
  }, []);

  useEffect(() => {
    let alive = true;
    fetch('/api/data')
      .then(r => r.json())
      .then(d => {
        if (!alive) return;
        const names = d?.agents?.manifest?.agents ? Object.keys(d.agents.manifest.agents) : [];
        setAgentNames(names);
      })
      .catch(() => {});
    return () => { alive = false; };
  }, []);

  const activeStep = useMemo(() => {
    if (!state) return null;
    return state.steps[state.current_step] || null;
  }, [state]);

  const isCheckpoint = activeStep?.type === 'checkpoint';
  const running = state?.status === 'running';

  const runPipeline = async (id: string) => {
    setErr(null);
    setBusy(true);
    try {
      await installPipeline(id, false); // idempotent
      const { state: st } = await startPipeline({
        id,
        started_by: meHumanId || null,
      });
      setState(st);
    } catch (e: any) {
      setErr(e?.message || 'falha ao iniciar');
    } finally {
      setBusy(false);
    }
  };

  const advance = async (decision: 'approve' | 'reject') => {
    if (!activeStep) return;
    setErr(null);
    setBusy(true);
    try {
      const { state: st } = await advancePipeline({
        step_id: activeStep.id,
        decision,
        note: note.trim() || undefined,
        decided_by: meHumanName || meHumanId || null,
      });
      setState(st);
      setNote('');
    } catch (e: any) {
      setErr(e?.message || 'falha ao avançar');
    } finally {
      setBusy(false);
    }
  };

  const cancelCurrent = async () => {
    if (!state) return;
    if (!window.confirm('Cancelar pipeline?')) return;
    setBusy(true);
    try {
      await cancelPipeline('cancelled by user');
      await refresh();
    } finally {
      setBusy(false);
    }
  };

  const clearCurrent = async () => {
    if (!window.confirm('Limpar histórico da pipeline?')) return;
    await clearPipeline();
    setState(null);
  };

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-slate-900/50 backdrop-blur-sm p-4">
      <div className="flex h-[85vh] w-full max-w-4xl flex-col overflow-hidden rounded-xl border bg-white shadow-2xl">
        <header className="flex items-center justify-between border-b px-4 py-3">
          <div>
            <div className="flex items-center gap-2 text-sm font-semibold">
              <Workflow className="h-4 w-4" /> Pipelines
            </div>
            <p className="text-xs text-muted-foreground">
              fluxos reproduzíveis com checkpoints humanos
            </p>
          </div>
          <Button size="icon" variant="ghost" className="h-7 w-7" onClick={onClose}>
            <X className="h-4 w-4" />
          </Button>
        </header>

        <div className="soft-scroll flex-1 overflow-y-auto px-4 py-3">
          {err && (
            <div className="mb-3 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-700">
              {err}
            </div>
          )}

          {state && (
            <section className="mb-5 rounded-md border-2 border-primary/30 bg-primary/5 p-3">
              <div className="flex items-start justify-between gap-2">
                <div>
                  <div className="flex items-center gap-1.5">
                    <CircleDot className="h-3.5 w-3.5 text-primary animate-pulse" />
                    <span className="text-sm font-semibold">{state.title}</span>
                    <Badge variant={state.status === 'running' ? 'success' : state.status === 'done' ? 'outline' : 'destructive'}>
                      {state.status}
                    </Badge>
                  </div>
                  <p className="mt-0.5 text-xs text-muted-foreground">
                    run: <span className="font-mono">{state.run_id}</span>
                    {state.started_by && ` · por ${state.started_by}`}
                  </p>
                </div>
                <div className="flex gap-1">
                  {running && (
                    <Button size="sm" variant="outline" onClick={cancelCurrent}>
                      <Square className="h-3 w-3" /> cancelar
                    </Button>
                  )}
                  {!running && (
                    <Button size="sm" variant="outline" onClick={clearCurrent}>
                      limpar
                    </Button>
                  )}
                </div>
              </div>

              {/* Step list */}
              <ol className="mt-3 space-y-1.5">
                {state.steps.map((s) => (
                  <li
                    key={s.id}
                    className={`flex items-start gap-2 rounded border px-2.5 py-1.5 text-xs ${
                      s.status === 'active' ? 'border-primary/60 bg-white shadow-sm' :
                      s.status === 'done'   ? 'border-emerald-200 bg-emerald-50/50' :
                      s.status === 'rejected' ? 'border-red-200 bg-red-50/50' :
                      'border-slate-200 bg-white/50'
                    }`}
                  >
                    <div className="mt-0.5">
                      {s.status === 'active'   && <CircleDot className="h-3.5 w-3.5 text-primary animate-pulse" />}
                      {s.status === 'done'     && <Check className="h-3.5 w-3.5 text-emerald-600" />}
                      {s.status === 'rejected' && <XCircle className="h-3.5 w-3.5 text-red-600" />}
                      {s.status === 'pending'  && <Clock className="h-3.5 w-3.5 text-slate-400" />}
                    </div>
                    <div className="flex-1">
                      <div className="flex items-center gap-1.5">
                        <span className="font-semibold">{s.label}</span>
                        {s.type === 'checkpoint' && (
                          <Badge variant="outline" className="text-xs uppercase">checkpoint</Badge>
                        )}
                        {s.agent && <Badge variant="outline" className="text-xs">@{s.agent}</Badge>}
                      </div>
                      {s.description && (
                        <p className="mt-0.5 text-xs text-slate-600">{s.description}</p>
                      )}
                      {s.best_practices && s.best_practices.length > 0 && (
                        <div className="mt-1 flex flex-wrap gap-1">
                          {s.best_practices.map((bp) => (
                            <Badge key={bp} variant="outline" className="text-xs">📚 {bp}</Badge>
                          ))}
                        </div>
                      )}
                      {s.note && (
                        <p className="mt-1 text-xs italic text-slate-500">
                          &ldquo;{s.note}&rdquo; — {s.decided_by || 'sem autor'}
                        </p>
                      )}
                    </div>
                  </li>
                ))}
              </ol>

              {/* Checkpoint gate */}
              {running && isCheckpoint && activeStep && (
                <div className="mt-4 rounded-md border-2 border-amber-300 bg-amber-50 p-3">
                  <div className="mb-2 flex items-center gap-1.5 text-xs font-semibold text-amber-900">
                    <AlertCircle className="h-3.5 w-3.5" />
                    Checkpoint aguarda decisão humana
                  </div>
                  <p className="text-xs text-amber-900">{activeStep.description}</p>
                  <textarea
                    className="mt-2 w-full rounded border border-amber-200 bg-white px-2 py-1.5 text-xs focus:outline-none"
                    rows={2}
                    placeholder="nota opcional (motivo, contexto...)"
                    value={note}
                    onChange={(e) => setNote(e.target.value)}
                  />
                  <div className="mt-2 flex justify-end gap-1.5">
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => advance('reject')}
                      disabled={busy}
                      className="gap-1"
                    >
                      <XCircle className="h-3.5 w-3.5" /> rejeitar
                    </Button>
                    <Button
                      size="sm"
                      onClick={() => advance('approve')}
                      disabled={busy}
                      className="gap-1 bg-emerald-600 hover:bg-emerald-700"
                    >
                      <Check className="h-3.5 w-3.5" /> aprovar e avançar
                    </Button>
                  </div>
                </div>
              )}

              {/* Advance for agent steps (manual for now) */}
              {running && !isCheckpoint && activeStep && (
                <div className="mt-4 rounded-md border border-slate-200 bg-white p-3">
                  <p className="mb-2 text-xs text-slate-600">
                    Step de agente <span className="font-mono">@{activeStep.agent}</span> — marque como concluído quando terminar.
                  </p>
                  <div className="flex justify-end">
                    <Button size="sm" onClick={() => advance('approve')} disabled={busy} className="gap-1">
                      <Check className="h-3.5 w-3.5" /> marcar como feito
                    </Button>
                  </div>
                </div>
              )}
            </section>
          )}

          {runs.length > 0 && (
            <section className="mb-5">
              <h3 className="mb-2 flex items-center gap-1.5 text-xs font-semibold uppercase tracking-wide text-slate-500">
                <History className="h-3 w-3" /> Execuções anteriores
                <span className="text-xs font-normal text-slate-400">({runs.length})</span>
              </h3>
              <ul className="space-y-1.5">
                {runs.map((r) => (
                  <RunRow key={r.run_id} run={r} />
                ))}
              </ul>
            </section>
          )}

          {!pipelines && !err && (
            <p className="py-10 text-center text-xs text-muted-foreground">carregando…</p>
          )}

          {pipelines && (
            <section>
              <div className="mb-2 flex items-center justify-between gap-2">
                <h3 className="text-xs font-semibold uppercase tracking-wide text-slate-500">
                  Pipelines disponíveis
                </h3>
                <div className="flex items-center gap-1.5">
                  {onOpenChat && agentNames.includes('pipeline_writer') && (
                    <Button
                      size="sm"
                      variant="outline"
                      className="gap-1 border-indigo-300 text-indigo-700 hover:bg-indigo-50"
                      onClick={() => onOpenChat('pipeline_writer')}
                      title="descreva um fluxo em linguagem natural; o pipeline_writer grava o YAML"
                    >
                      <Sparkles className="h-3.5 w-3.5" /> pedir ao agente
                    </Button>
                  )}
                  <Button size="sm" variant="outline" className="gap-1" onClick={() => setNewOpen(true)}>
                    <Plus className="h-3.5 w-3.5" /> nova pipeline
                  </Button>
                </div>
              </div>
              <div className="space-y-2">
                {pipelines.map((p) => (
                  <div key={p.id} className="rounded-md border">
                    <div className="flex items-start justify-between gap-2 p-3">
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-1.5">
                          <span className="text-sm font-semibold">{p.label}</span>
                          <Badge variant="outline" className="text-xs">{p.stepCount} steps</Badge>
                          {p.source === 'installed' && (
                            <Badge variant="outline" className="text-xs">instalado</Badge>
                          )}
                        </div>
                        {p.description && (
                          <p className="mt-0.5 whitespace-pre-line text-xs text-slate-600">
                            {p.description.trim().slice(0, 220)}
                          </p>
                        )}
                        {p.tags && p.tags.length > 0 && (
                          <div className="mt-1 flex flex-wrap gap-1">
                            {p.tags.map((t) => (
                              <Badge key={t} variant="outline" className="text-xs">{t}</Badge>
                            ))}
                          </div>
                        )}
                      </div>
                      <Button
                        size="sm"
                        className="shrink-0 gap-1.5"
                        onClick={() => runPipeline(p.id)}
                        disabled={busy || running}
                      >
                        <Play className="h-3.5 w-3.5" /> rodar
                      </Button>
                    </div>
                    <details className="border-t px-3 py-2 text-xs text-slate-600">
                      <summary className="cursor-pointer select-none">
                        <ChevronRight className="inline h-3 w-3" /> ver steps
                      </summary>
                      <ol className="mt-2 space-y-1">
                        {p.steps.map((s, i) => (
                          <li key={s.id} className="flex gap-2">
                            <span className="text-slate-400">{i + 1}.</span>
                            <span>
                              <span className="font-semibold">{s.label || s.id}</span>
                              {s.type === 'checkpoint' && (
                                <Badge variant="outline" className="ml-1.5 text-xs uppercase">checkpoint</Badge>
                              )}
                              {s.agent && <span className="ml-1 font-mono text-slate-500">@{s.agent}</span>}
                            </span>
                          </li>
                        ))}
                      </ol>
                    </details>
                  </div>
                ))}
                {pipelines.length === 0 && (
                  <p className="rounded border border-dashed p-4 text-center text-xs text-slate-500">
                    Nenhuma pipeline ainda. Adicione arquivos YAML em plugin/pipelines/&lt;id&gt;/pipeline.yaml
                  </p>
                )}
              </div>
            </section>
          )}
        </div>
      </div>
      {newOpen && (
        <NewPipelineForm
          agentNames={agentNames}
          existingIds={new Set((pipelines || []).map(p => p.id))}
          onClose={() => setNewOpen(false)}
          onCreated={() => {
            setNewOpen(false);
            refresh();
          }}
        />
      )}
    </div>
  );
}

const STATUS_STYLE: Record<PipelineStatus, string> = {
  running: 'border-primary/40 bg-primary/10 text-primary',
  done: 'border-emerald-200 bg-emerald-50 text-emerald-700',
  aborted: 'border-red-200 bg-red-50 text-red-700',
  cancelled: 'border-slate-200 bg-slate-100 text-slate-600',
};

function formatDuration(ms: number | null): string {
  if (ms === null || !Number.isFinite(ms)) return '—';
  if (ms < 1000) return `${ms}ms`;
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  const rs = s % 60;
  if (m < 60) return `${m}m${rs ? ` ${rs}s` : ''}`;
  const h = Math.floor(m / 60);
  const rm = m % 60;
  return `${h}h${rm ? ` ${rm}m` : ''}`;
}

function RunRow({ run }: { run: PipelineRunSummary }) {
  const style = STATUS_STYLE[run.status] || STATUS_STYLE.done;
  const when = run.started_at ? new Date(run.started_at) : null;
  const whenLabel = when
    ? `${when.toLocaleDateString()} ${when.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}`
    : '—';
  return (
    <li className="rounded-md border border-slate-200 bg-white px-3 py-2 text-xs">
      <div className="flex items-center justify-between gap-2">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1.5">
            <span className={`rounded border px-1.5 py-0 text-xs font-semibold uppercase ${style}`}>
              {run.status}
            </span>
            <span className="truncate font-semibold">{run.title || run.pipeline_id}</span>
            <span className="shrink-0 font-mono text-xs text-slate-400">{run.pipeline_id}</span>
          </div>
          <div className="mt-0.5 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-xs text-slate-500">
            <span>{whenLabel}</span>
            <span>·</span>
            <span>{run.steps_done}/{run.total_steps} steps</span>
            {run.steps_rejected > 0 && (
              <>
                <span>·</span>
                <span className="text-red-600">{run.steps_rejected} rejeitados</span>
              </>
            )}
            <span>·</span>
            <span>{formatDuration(run.duration_ms)}</span>
            {run.started_by && (
              <>
                <span>·</span>
                <span>por {run.started_by}</span>
              </>
            )}
          </div>
          {run.cancel_reason && (
            <p className="mt-0.5 text-xs italic text-slate-500">motivo: {run.cancel_reason}</p>
          )}
        </div>
      </div>
    </li>
  );
}

interface StepDraft {
  id: string;
  type: PipelineStepType;
  agent: string;
  label: string;
  description: string;
  owner: string;
  best_practices: string;
}

function emptyStep(): StepDraft {
  return { id: '', type: 'agent', agent: '', label: '', description: '', owner: '', best_practices: '' };
}

interface NewPipelineFormProps {
  agentNames: string[];
  existingIds: Set<string>;
  onClose: () => void;
  onCreated: () => void;
}

function slugify(s: string): string {
  return s
    .toLowerCase()
    .normalize('NFD').replace(/[\u0300-\u036f]/g, '')
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 50);
}

function NewPipelineForm({ agentNames, existingIds, onClose, onCreated }: NewPipelineFormProps) {
  const [id, setId] = useState('');
  const [idTouched, setIdTouched] = useState(false);
  const [label, setLabel] = useState('');
  const [description, setDescription] = useState('');
  const [tagsRaw, setTagsRaw] = useState('');
  const [scope, setScope] = useState<'project' | 'template'>('project');
  const [steps, setSteps] = useState<StepDraft[]>([{ ...emptyStep(), id: 'step-1' }]);
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const onLabelChange = (v: string) => {
    setLabel(v);
    if (!idTouched) setId(slugify(v));
  };

  const updateStep = (i: number, patch: Partial<StepDraft>) => {
    setSteps((prev) => prev.map((s, idx) => (idx === i ? { ...s, ...patch } : s)));
  };
  const addStep = () => {
    setSteps((prev) => [...prev, { ...emptyStep(), id: `step-${prev.length + 1}` }]);
  };
  const removeStep = (i: number) => {
    setSteps((prev) => (prev.length > 1 ? prev.filter((_, idx) => idx !== i) : prev));
  };
  const moveStep = (i: number, dir: -1 | 1) => {
    setSteps((prev) => {
      const j = i + dir;
      if (j < 0 || j >= prev.length) return prev;
      const next = [...prev];
      [next[i], next[j]] = [next[j], next[i]];
      return next;
    });
  };

  const save = async () => {
    setErr(null);
    if (!id) return setErr('id obrigatório');
    if (!label) return setErr('label obrigatório');
    if (existingIds.has(id)) return setErr(`já existe uma pipeline com id "${id}"`);

    const stepDefs: PipelineStepDef[] = steps.map((s) => {
      const d: PipelineStepDef = { id: s.id, type: s.type };
      if (s.type === 'agent' && s.agent) d.agent = s.agent;
      if (s.label.trim()) d.label = s.label.trim();
      if (s.description.trim()) d.description = s.description.trim();
      if (s.type === 'checkpoint' && s.owner.trim()) d.owner = s.owner.trim();
      const bp = s.best_practices.split(',').map(x => x.trim()).filter(Boolean);
      if (bp.length) d.best_practices = bp;
      return d;
    });

    setSaving(true);
    try {
      await createPipeline({
        id,
        label,
        description: description.trim() || undefined,
        tags: tagsRaw.split(',').map(t => t.trim()).filter(Boolean),
        steps: stepDefs,
        scope,
      });
      onCreated();
    } catch (e: any) {
      setErr(e?.message || 'falha ao criar');
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="fixed inset-0 z-[60] flex items-center justify-center bg-slate-900/60 backdrop-blur-sm p-4">
      <div className="flex max-h-[90vh] w-full max-w-2xl flex-col overflow-hidden rounded-xl border bg-white shadow-2xl">
        <header className="flex items-center justify-between border-b px-4 py-3">
          <div>
            <div className="flex items-center gap-2 text-sm font-semibold">
              <Workflow className="h-4 w-4" /> Nova pipeline
            </div>
            <p className="text-xs text-muted-foreground">
              descreva os passos; humanos aprovam em checkpoints
            </p>
          </div>
          <Button size="icon" variant="ghost" className="h-7 w-7" onClick={onClose}>
            <X className="h-4 w-4" />
          </Button>
        </header>

        <div className="soft-scroll flex-1 overflow-y-auto p-4 text-xs">
          <div className="mb-3 grid grid-cols-2 gap-2">
            <label className="flex flex-col gap-1">
              <span className="text-xs font-semibold uppercase text-slate-600">Label</span>
              <input
                value={label}
                onChange={(e) => onLabelChange(e.target.value)}
                placeholder="ex: Revisar antes de mergear"
                className="rounded border border-slate-200 px-2 py-1.5 focus:border-indigo-400 focus:outline-none"
              />
            </label>
            <label className="flex flex-col gap-1">
              <span className="text-xs font-semibold uppercase text-slate-600">
                ID <span className="text-slate-400">(slug)</span>
              </span>
              <input
                value={id}
                onChange={(e) => { setIdTouched(true); setId(e.target.value); }}
                placeholder="review-before-merge"
                className="rounded border border-slate-200 px-2 py-1.5 font-mono focus:border-indigo-400 focus:outline-none"
              />
            </label>
          </div>

          <label className="mb-3 flex flex-col gap-1">
            <span className="text-xs font-semibold uppercase text-slate-600">Descrição</span>
            <textarea
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              rows={2}
              placeholder="pra que serve este fluxo, quando usar…"
              className="rounded border border-slate-200 px-2 py-1.5 focus:border-indigo-400 focus:outline-none"
            />
          </label>

          <div className="mb-3 grid grid-cols-2 gap-2">
            <label className="flex flex-col gap-1">
              <span className="text-xs font-semibold uppercase text-slate-600">
                Tags <span className="text-slate-400">(vírgula)</span>
              </span>
              <input
                value={tagsRaw}
                onChange={(e) => setTagsRaw(e.target.value)}
                placeholder="content, marketing"
                className="rounded border border-slate-200 px-2 py-1.5 focus:border-indigo-400 focus:outline-none"
              />
            </label>
            <label className="flex flex-col gap-1">
              <span className="text-xs font-semibold uppercase text-slate-600">Escopo</span>
              <select
                value={scope}
                onChange={(e) => setScope(e.target.value as 'project' | 'template')}
                className="rounded border border-slate-200 bg-white px-2 py-1.5 focus:border-indigo-400 focus:outline-none"
              >
                <option value="project">só este projeto</option>
                <option value="template">template global (todos os projetos)</option>
              </select>
            </label>
          </div>

          <div className="mb-2 flex items-center justify-between">
            <span className="text-xs font-semibold uppercase text-slate-600">Passos</span>
            <Button size="sm" variant="outline" className="h-7 gap-1 text-xs" onClick={addStep}>
              <Plus className="h-3 w-3" /> passo
            </Button>
          </div>

          <ol className="space-y-2">
            {steps.map((s, i) => (
              <li key={i} className="rounded-md border border-slate-200 bg-slate-50/50 p-2">
                <div className="mb-1.5 flex items-center justify-between">
                  <span className="text-xs font-semibold text-slate-500">#{i + 1}</span>
                  <div className="flex items-center gap-0.5">
                    <button
                      onClick={() => moveStep(i, -1)}
                      disabled={i === 0}
                      className="rounded px-1 py-0.5 text-xs text-slate-500 hover:bg-slate-200 disabled:opacity-30"
                    >↑</button>
                    <button
                      onClick={() => moveStep(i, 1)}
                      disabled={i === steps.length - 1}
                      className="rounded px-1 py-0.5 text-xs text-slate-500 hover:bg-slate-200 disabled:opacity-30"
                    >↓</button>
                    <button
                      onClick={() => removeStep(i)}
                      disabled={steps.length <= 1}
                      className="rounded px-1 py-0.5 text-slate-400 hover:bg-red-100 hover:text-red-600 disabled:opacity-30"
                      title="remover passo"
                    >
                      <Trash2 className="h-3 w-3" />
                    </button>
                  </div>
                </div>
                <div className="grid grid-cols-3 gap-1.5">
                  <label className="flex flex-col gap-0.5">
                    <span className="text-xs uppercase text-slate-500">ID</span>
                    <input
                      value={s.id}
                      onChange={(e) => updateStep(i, { id: e.target.value })}
                      placeholder="research"
                      className="rounded border border-slate-200 px-1.5 py-1 font-mono text-xs focus:border-indigo-400 focus:outline-none"
                    />
                  </label>
                  <label className="flex flex-col gap-0.5">
                    <span className="text-xs uppercase text-slate-500">Tipo</span>
                    <select
                      value={s.type}
                      onChange={(e) => updateStep(i, { type: e.target.value as PipelineStepType })}
                      className="rounded border border-slate-200 bg-white px-1.5 py-1 text-xs focus:border-indigo-400 focus:outline-none"
                    >
                      <option value="agent">agent</option>
                      <option value="checkpoint">checkpoint</option>
                    </select>
                  </label>
                  <label className="flex flex-col gap-0.5">
                    <span className="text-xs uppercase text-slate-500">
                      {s.type === 'checkpoint' ? 'Owner' : 'Agent'}
                    </span>
                    {s.type === 'agent' ? (
                      <>
                        <input
                          list={`agents-${i}`}
                          value={s.agent}
                          onChange={(e) => updateStep(i, { agent: e.target.value })}
                          placeholder="manager"
                          className="rounded border border-slate-200 px-1.5 py-1 font-mono text-xs focus:border-indigo-400 focus:outline-none"
                        />
                        <datalist id={`agents-${i}`}>
                          {agentNames.map((n) => <option key={n} value={n} />)}
                        </datalist>
                      </>
                    ) : (
                      <input
                        value={s.owner}
                        onChange={(e) => updateStep(i, { owner: e.target.value })}
                        placeholder="any_human"
                        className="rounded border border-slate-200 px-1.5 py-1 text-xs focus:border-indigo-400 focus:outline-none"
                      />
                    )}
                  </label>
                </div>
                <label className="mt-1.5 flex flex-col gap-0.5">
                  <span className="text-xs uppercase text-slate-500">Label</span>
                  <input
                    value={s.label}
                    onChange={(e) => updateStep(i, { label: e.target.value })}
                    placeholder="Pesquisar tema"
                    className="rounded border border-slate-200 px-1.5 py-1 text-xs focus:border-indigo-400 focus:outline-none"
                  />
                </label>
                <label className="mt-1 flex flex-col gap-0.5">
                  <span className="text-xs uppercase text-slate-500">Descrição</span>
                  <textarea
                    value={s.description}
                    onChange={(e) => updateStep(i, { description: e.target.value })}
                    rows={1}
                    placeholder="o que acontece neste passo…"
                    className="rounded border border-slate-200 px-1.5 py-1 text-xs focus:border-indigo-400 focus:outline-none"
                  />
                </label>
                {s.type === 'agent' && (
                  <label className="mt-1 flex flex-col gap-0.5">
                    <span className="text-xs uppercase text-slate-500">
                      Best practices <span className="text-slate-400">(vírgula)</span>
                    </span>
                    <input
                      value={s.best_practices}
                      onChange={(e) => updateStep(i, { best_practices: e.target.value })}
                      placeholder="copywriting, accessibility"
                      className="rounded border border-slate-200 px-1.5 py-1 text-xs focus:border-indigo-400 focus:outline-none"
                    />
                  </label>
                )}
              </li>
            ))}
          </ol>

          {err && (
            <div className="mt-3 rounded border border-red-200 bg-red-50 px-2 py-1.5 text-xs text-red-700">
              {err}
            </div>
          )}
        </div>

        <footer className="flex justify-end gap-2 border-t px-4 py-3">
          <Button size="sm" variant="outline" onClick={onClose} disabled={saving}>
            cancelar
          </Button>
          <Button size="sm" onClick={save} disabled={saving} className="gap-1.5">
            <Check className="h-3.5 w-3.5" />
            {saving ? 'criando…' : 'criar pipeline'}
          </Button>
        </footer>
      </div>
    </div>
  );
}
