import { useEffect, useMemo, useState } from 'react';
import { X, ShieldAlert, Lock, Plus, Trash2, Save, FlaskConical, AlertTriangle } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import {
  listPatterns,
  saveUserPatterns,
  testPattern,
  toggleCorePattern,
  type Pattern,
  type PatternSeverity,
  type PatternKind,
  type DetectResult,
} from '@/lib/destructive';

interface Props {
  onClose: () => void;
}

function sevColor(s: PatternSeverity) {
  if (s === 'critical') return 'text-red-600 bg-red-50 border-red-200';
  if (s === 'high') return 'text-orange-600 bg-orange-50 border-orange-200';
  return 'text-amber-600 bg-amber-50 border-amber-200';
}

function sevLabel(s: PatternSeverity) {
  return s === 'critical' ? 'crítico' : s === 'high' ? 'alto' : 'médio';
}

function newUserPattern(): Pattern {
  return {
    id: '',
    label: '',
    text: '',
    kind: 'literal',
    severity: 'high',
    enabled: true,
    origin: 'user',
  };
}

export function ProtectionsPanel({ onClose }: Props) {
  const [core, setCore] = useState<Pattern[]>([]);
  const [user, setUser] = useState<Pattern[]>([]);
  const [loading, setLoading] = useState(true);
  const [err, setErr] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [editing, setEditing] = useState<Pattern | null>(null);
  const [testText, setTestText] = useState('');
  const [testResult, setTestResult] = useState<DetectResult | null>(null);
  const [testing, setTesting] = useState(false);

  const reload = async () => {
    setLoading(true);
    try {
      const j = await listPatterns();
      setCore(j.core || []);
      setUser(j.user || []);
    } catch (e: any) {
      setErr(e?.message || 'falha ao listar');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => { reload(); }, []);

  const saveEditing = async () => {
    if (!editing) return;
    if (!editing.label.trim() || !editing.text.trim()) {
      setErr('preencha label e texto do pattern');
      return;
    }
    setSaving(true);
    setErr(null);
    try {
      const nextList = editing.id
        ? user.map((p) => (p.id === editing.id ? editing : p))
        : [...user, { ...editing, id: `user.${crypto.randomUUID().slice(0, 8)}` }];
      const saved = await saveUserPatterns(nextList);
      setUser(saved);
      setEditing(null);
    } catch (e: any) {
      setErr(e?.message || 'falha ao salvar');
    } finally {
      setSaving(false);
    }
  };

  const remove = async (id: string) => {
    if (!confirm('Remover este pattern de proteção?')) return;
    setSaving(true);
    try {
      const saved = await saveUserPatterns(user.filter((p) => p.id !== id));
      setUser(saved);
    } catch (e: any) {
      setErr(e?.message || 'falha ao remover');
    } finally {
      setSaving(false);
    }
  };

  const toggleEnabled = async (id: string) => {
    setSaving(true);
    try {
      const saved = await saveUserPatterns(user.map((p) => p.id === id ? { ...p, enabled: !p.enabled } : p));
      setUser(saved);
    } catch (e: any) {
      setErr(e?.message || 'falha');
    } finally {
      setSaving(false);
    }
  };

  const toggleCoreEnabled = async (p: Pattern) => {
    const willDisable = p.enabled;
    if (willDisable && p.severity === 'critical') {
      const ok = confirm(
        `Desabilitar proteção crítica "${p.label}"?\n\nIsso deixa passar pedidos que casariam com esse pattern. O internal_firefighter registra cada toggle em log de auditoria.`
      );
      if (!ok) return;
    }
    setSaving(true);
    try {
      await toggleCorePattern(p.id, willDisable);
      setCore((cur) => cur.map((x) => (x.id === p.id ? { ...x, enabled: !willDisable } : x)));
    } catch (e: any) {
      setErr(e?.message || 'falha ao alterar pattern core');
    } finally {
      setSaving(false);
    }
  };

  const runTest = async () => {
    if (!testText.trim()) { setTestResult(null); return; }
    setTesting(true);
    try {
      const r = await testPattern(testText);
      setTestResult(r);
    } catch (e: any) {
      setErr(e?.message || 'falha ao testar');
    } finally {
      setTesting(false);
    }
  };

  const coreBySev = useMemo(() => {
    const order: PatternSeverity[] = ['critical', 'high', 'medium'];
    return [...core].sort((a, b) => order.indexOf(a.severity) - order.indexOf(b.severity));
  }, [core]);

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-slate-900/50 backdrop-blur-sm p-4">
      <div className="flex h-[85vh] w-full max-w-5xl flex-col overflow-hidden rounded-xl border bg-white shadow-2xl">
        <header className="flex items-center justify-between border-b px-4 py-3">
          <div>
            <div className="flex items-center gap-2 text-sm font-semibold">
              <ShieldAlert className="h-4 w-4 text-red-600" /> Proteções — motor de intent destrutivo
            </div>
            <p className="mt-0.5 text-xs text-muted-foreground">
              Mensagens que casem com qualquer pattern abaixo são bloqueadas antes de chegar ao agente — o internal_firefighter é acionado automaticamente.
            </p>
          </div>
          <Button size="icon" variant="ghost" onClick={onClose}><X className="h-4 w-4" /></Button>
        </header>

        {err && (
          <div className="mx-4 mt-3 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-700">
            {err}
          </div>
        )}

        <div className="grid flex-1 grid-cols-[1fr_320px] overflow-hidden">
          <div className="overflow-y-auto p-4">
            {loading ? (
              <p className="text-sm text-muted-foreground">Carregando…</p>
            ) : (
              <>
                <section className="mb-6">
                  <div className="mb-2 flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-slate-500">
                    <Lock className="h-3 w-3" /> Patterns CORE ({coreBySev.length}) — texto imutável, pode desabilitar
                  </div>
                  <div className="space-y-1.5">
                    {coreBySev.map((p) => (
                      <div
                        key={p.id}
                        className={`rounded-md border px-3 py-2 text-sm ${
                          p.enabled
                            ? 'border-slate-200 bg-slate-50'
                            : 'border-dashed border-slate-300 bg-white opacity-60'
                        }`}
                      >
                        <div className="flex items-start justify-between gap-2">
                          <div className="min-w-0 flex-1">
                            <div className={`font-medium ${p.enabled ? '' : 'line-through text-slate-500'}`}>{p.label}</div>
                            <code className="mt-0.5 block truncate font-mono text-xs text-slate-500">{p.text}</code>
                          </div>
                          <div className="flex shrink-0 items-center gap-1">
                            <span className={`rounded border px-1.5 py-0.5 text-xs font-medium ${sevColor(p.severity)}`}>
                              {sevLabel(p.severity)}
                            </span>
                            <Button
                              size="icon"
                              variant="ghost"
                              className={`h-6 w-6 ${p.enabled ? 'text-emerald-600' : 'text-slate-400'}`}
                              onClick={() => toggleCoreEnabled(p)}
                              disabled={saving}
                              title={p.enabled ? 'desabilitar este pattern' : 'reabilitar este pattern'}
                            >
                              {p.enabled ? '●' : '○'}
                            </Button>
                          </div>
                        </div>
                      </div>
                    ))}
                  </div>
                </section>

                <section>
                  <div className="mb-2 flex items-center justify-between">
                    <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-slate-500">
                      Patterns do usuário ({user.length})
                    </div>
                    <Button size="sm" onClick={() => setEditing(newUserPattern())}>
                      <Plus className="mr-1 h-3.5 w-3.5" /> novo
                    </Button>
                  </div>
                  {user.length === 0 ? (
                    <p className="rounded border border-dashed border-slate-300 p-4 text-center text-xs text-slate-500">
                      Nenhum pattern customizado. Os core já cobrem os casos clássicos — adicione aqui só se tiver risco específico do seu projeto.
                    </p>
                  ) : (
                    <div className="space-y-1.5">
                      {user.map((p) => (
                        <div key={p.id} className={`rounded-md border px-3 py-2 text-sm ${p.enabled ? 'border-slate-200 bg-white' : 'border-slate-200 bg-slate-50 opacity-60'}`}>
                          <div className="flex items-start justify-between gap-2">
                            <div className="min-w-0 flex-1">
                              <div className="font-medium">{p.label}</div>
                              <code className="mt-0.5 block truncate font-mono text-xs text-slate-500">
                                {p.kind === 'regex' ? `/${p.text}/i` : p.text}
                              </code>
                            </div>
                            <div className="flex shrink-0 items-center gap-1">
                              <span className={`rounded border px-1.5 py-0.5 text-xs font-medium ${sevColor(p.severity)}`}>
                                {sevLabel(p.severity)}
                              </span>
                              <Badge variant="outline" className="text-xs">{p.kind}</Badge>
                              <Button size="icon" variant="ghost" className="h-6 w-6" onClick={() => toggleEnabled(p.id)} title={p.enabled ? 'desabilitar' : 'habilitar'}>
                                {p.enabled ? '●' : '○'}
                              </Button>
                              <Button size="icon" variant="ghost" className="h-6 w-6" onClick={() => setEditing(p)} title="editar">
                                <Save className="h-3 w-3" />
                              </Button>
                              <Button size="icon" variant="ghost" className="h-6 w-6 text-red-600" onClick={() => remove(p.id)} title="remover">
                                <Trash2 className="h-3 w-3" />
                              </Button>
                            </div>
                          </div>
                        </div>
                      ))}
                    </div>
                  )}
                </section>
              </>
            )}
          </div>

          <aside className="flex flex-col border-l bg-slate-50">
            {editing ? (
              <div className="flex-1 overflow-y-auto p-4">
                <div className="mb-3 text-xs font-semibold uppercase tracking-wide text-slate-500">
                  {editing.id ? 'Editar pattern' : 'Novo pattern'}
                </div>
                <div className="space-y-2.5 text-sm">
                  <div>
                    <label className="mb-1 block text-xs font-semibold text-slate-700">Rótulo</label>
                    <input
                      type="text"
                      value={editing.label}
                      onChange={(e) => setEditing({ ...editing, label: e.target.value })}
                      className="w-full rounded border border-slate-300 bg-white px-2 py-1.5 text-sm text-slate-900 placeholder:text-slate-400 focus:border-indigo-400 focus:outline-none"
                      placeholder="ex: apagar dados de produção"
                    />
                  </div>
                  <div>
                    <label className="mb-1 block text-xs font-semibold text-slate-700">Tipo</label>
                    <div className="flex gap-3 text-xs text-slate-700">
                      <label className="flex items-center gap-1.5">
                        <input type="radio" checked={editing.kind === 'literal'} onChange={() => setEditing({ ...editing, kind: 'literal' })} />
                        literal (use <code>*</code> como wildcard)
                      </label>
                      <label className="flex items-center gap-1.5">
                        <input type="radio" checked={editing.kind === 'regex'} onChange={() => setEditing({ ...editing, kind: 'regex' })} />
                        regex
                      </label>
                    </div>
                  </div>
                  <div>
                    <label className="mb-1 block text-xs font-semibold text-slate-700">Texto</label>
                    <input
                      type="text"
                      value={editing.text}
                      onChange={(e) => setEditing({ ...editing, text: e.target.value })}
                      className="w-full rounded border border-slate-300 bg-white px-2 py-1.5 font-mono text-sm text-slate-900 placeholder:text-slate-400 focus:border-indigo-400 focus:outline-none"
                      placeholder={editing.kind === 'regex' ? '\\bapagar\\s+dados\\b' : 'apagar * produção'}
                    />
                  </div>
                  <div>
                    <label className="mb-1 block text-xs font-semibold text-slate-700">Severidade</label>
                    <select
                      value={editing.severity}
                      onChange={(e) => setEditing({ ...editing, severity: e.target.value as PatternSeverity })}
                      className="w-full rounded border border-slate-300 bg-white px-2 py-1.5 text-sm text-slate-900 focus:border-indigo-400 focus:outline-none"
                    >
                      <option value="critical">crítico</option>
                      <option value="high">alto</option>
                      <option value="medium">médio</option>
                    </select>
                  </div>
                  <div className="flex items-center gap-2 pt-1">
                    <input type="checkbox" id="en" checked={editing.enabled} onChange={(e) => setEditing({ ...editing, enabled: e.target.checked })} />
                    <label htmlFor="en" className="text-xs text-slate-700">habilitado</label>
                  </div>
                  <div className="flex gap-2 pt-2">
                    <Button size="sm" onClick={saveEditing} disabled={saving}>
                      <Save className="mr-1 h-3.5 w-3.5" /> salvar
                    </Button>
                    <Button size="sm" variant="outline" onClick={() => setEditing(null)} disabled={saving}>
                      cancelar
                    </Button>
                  </div>
                </div>
              </div>
            ) : (
              <div className="flex-1 overflow-y-auto p-4">
                <div className="mb-2 flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-slate-500">
                  <FlaskConical className="h-3 w-3" /> Testar mensagem
                </div>
                <textarea
                  value={testText}
                  onChange={(e) => setTestText(e.target.value)}
                  placeholder="cole uma mensagem aqui pra ver quais patterns casam"
                  rows={5}
                  className="w-full resize-y rounded border border-slate-300 bg-white px-2 py-1.5 text-sm text-slate-900 placeholder:text-slate-400 focus:border-indigo-400 focus:outline-none"
                />
                <Button size="sm" onClick={runTest} disabled={testing} className="mt-2">
                  {testing ? 'testando…' : 'testar'}
                </Button>
                {testResult && (
                  <div className="mt-3">
                    {testResult.matched ? (
                      <div className="rounded border border-red-200 bg-red-50 p-2 text-xs">
                        <div className="mb-1 flex items-center gap-1 font-semibold text-red-700">
                          <AlertTriangle className="h-3 w-3" /> {testResult.hits.length} match(es) — mensagem seria BLOQUEADA
                        </div>
                        <ul className="space-y-0.5 text-red-800">
                          {testResult.hits.map((h) => (
                            <li key={h.id}>
                              <Badge variant="outline" className="mr-1 text-xs">{h.origin}</Badge>
                              {h.label} → <code className="font-mono text-xs">{h.match}</code>
                            </li>
                          ))}
                        </ul>
                      </div>
                    ) : (
                      <div className="rounded border border-emerald-200 bg-emerald-50 p-2 text-xs text-emerald-700">
                        ✓ nenhum pattern casou — mensagem seguiria normal
                      </div>
                    )}
                  </div>
                )}
              </div>
            )}
          </aside>
        </div>
      </div>
    </div>
  );
}
