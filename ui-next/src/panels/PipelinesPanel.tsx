import { useEffect, useMemo, useState } from 'react';
import { X, Workflow, Play, Check, XCircle, ChevronRight, Clock, Square, CircleDot, AlertCircle } from 'lucide-react';
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
  type PipelineDef,
  type PipelineRunState,
} from '@/lib/pipelines';

interface Props {
  onClose: () => void;
  meHumanId: string | null;
  meHumanName: string | null;
}

export function PipelinesPanel({ onClose, meHumanId, meHumanName }: Props) {
  const [pipelines, setPipelines] = useState<PipelineDef[] | null>(null);
  const [state, setState] = useState<PipelineRunState | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState('');

  const refresh = async () => {
    try {
      const [{ pipelines: list }, { state: st }] = await Promise.all([
        listPipelines(),
        getPipelineState(),
      ]);
      setPipelines(list);
      setState(st);
    } catch (e: any) {
      setErr(e?.message || 'falha ao carregar');
    }
  };

  useEffect(() => {
    refresh();
    const t = setInterval(refresh, 3000);
    return () => clearInterval(t);
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
            <p className="text-[11px] text-muted-foreground">
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
                  <p className="mt-0.5 text-[11px] text-muted-foreground">
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
                          <Badge variant="outline" className="text-[9px] uppercase">checkpoint</Badge>
                        )}
                        {s.agent && <Badge variant="outline" className="text-[9px]">@{s.agent}</Badge>}
                      </div>
                      {s.description && (
                        <p className="mt-0.5 text-[11px] text-slate-600">{s.description}</p>
                      )}
                      {s.best_practices && s.best_practices.length > 0 && (
                        <div className="mt-1 flex flex-wrap gap-1">
                          {s.best_practices.map((bp) => (
                            <Badge key={bp} variant="outline" className="text-[9px]">📚 {bp}</Badge>
                          ))}
                        </div>
                      )}
                      {s.note && (
                        <p className="mt-1 text-[11px] italic text-slate-500">
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

          {!pipelines && !err && (
            <p className="py-10 text-center text-xs text-muted-foreground">carregando…</p>
          )}

          {pipelines && (
            <section>
              <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-slate-500">
                Pipelines disponíveis
              </h3>
              <div className="space-y-2">
                {pipelines.map((p) => (
                  <div key={p.id} className="rounded-md border">
                    <div className="flex items-start justify-between gap-2 p-3">
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-1.5">
                          <span className="text-sm font-semibold">{p.label}</span>
                          <Badge variant="outline" className="text-[9px]">{p.stepCount} steps</Badge>
                          {p.source === 'installed' && (
                            <Badge variant="outline" className="text-[9px]">instalado</Badge>
                          )}
                        </div>
                        {p.description && (
                          <p className="mt-0.5 whitespace-pre-line text-[11px] text-slate-600">
                            {p.description.trim().slice(0, 220)}
                          </p>
                        )}
                        {p.tags && p.tags.length > 0 && (
                          <div className="mt-1 flex flex-wrap gap-1">
                            {p.tags.map((t) => (
                              <Badge key={t} variant="outline" className="text-[9px]">{t}</Badge>
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
                    <details className="border-t px-3 py-2 text-[11px] text-slate-600">
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
                                <Badge variant="outline" className="ml-1.5 text-[9px] uppercase">checkpoint</Badge>
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
    </div>
  );
}
