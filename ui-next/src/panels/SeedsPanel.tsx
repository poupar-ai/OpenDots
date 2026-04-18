import { useEffect, useMemo, useState } from 'react';
import { X, Package, Check, AlertCircle, ChevronRight, ChevronDown } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import {
  listSeeds,
  applySeed,
  type SeedCategory,
  type SeedApplyResult,
} from '@/lib/seeds';

interface Props {
  onClose: () => void;
  onInstalled?: () => void;
}

export function SeedsPanel({ onClose, onInstalled }: Props) {
  const [categories, setCategories] = useState<SeedCategory[] | null>(null);
  const [projectRoot, setProjectRoot] = useState<string>('');
  const [loadErr, setLoadErr] = useState<string | null>(null);

  const [expandedPack, setExpandedPack] = useState<string | null>(null);
  const [picked, setPicked] = useState<Record<string, Set<string>>>({});
  const [overwrite, setOverwrite] = useState(false);
  const [applying, setApplying] = useState<string | null>(null);
  const [result, setResult] = useState<{ ref: string; r: SeedApplyResult } | null>(null);
  const [applyErr, setApplyErr] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const j = await listSeeds();
        if (!alive) return;
        setCategories(j.categories);
        setProjectRoot(j.project_root);
        if (j.categories.length === 1 && j.categories[0].packs.length === 1) {
          setExpandedPack(`${j.categories[0].name}/${j.categories[0].packs[0].name}`);
        }
      } catch (e: any) {
        if (alive) setLoadErr(e?.message || 'falha ao carregar');
      }
    })();
    return () => { alive = false; };
  }, []);

  function toggleAgent(ref: string, agent: string, allAgents: string[]) {
    setPicked((prev) => {
      const cur = new Set(prev[ref] ?? allAgents);
      if (cur.has(agent)) cur.delete(agent);
      else cur.add(agent);
      return { ...prev, [ref]: cur };
    });
  }

  function getPicked(ref: string, allAgents: string[]): Set<string> {
    return picked[ref] ?? new Set(allAgents);
  }

  function resetPicked(ref: string, allAgents: string[]) {
    setPicked((prev) => ({ ...prev, [ref]: new Set(allAgents) }));
  }

  async function install(ref: string, allAgents: string[]) {
    setApplying(ref);
    setApplyErr(null);
    setResult(null);
    try {
      const sel = getPicked(ref, allAgents);
      const only = sel.size === allAgents.length ? undefined : Array.from(sel);
      const r = await applySeed({ ref, only, overwrite });
      setResult({ ref, r });
      onInstalled?.();
    } catch (e: any) {
      setApplyErr(e?.message || 'falha ao instalar');
    } finally {
      setApplying(null);
    }
  }

  const summary = useMemo(() => {
    if (!result) return null;
    const { r } = result;
    const parts: string[] = [];
    if (r.added.length)    parts.push(`${r.added.length} adicionados`);
    if (r.replaced.length) parts.push(`${r.replaced.length} substituídos`);
    if (r.skipped.length)  parts.push(`${r.skipped.length} preservados`);
    return parts.join(' · ');
  }, [result]);

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-slate-900/50 backdrop-blur-sm p-4">
      <div className="flex h-[85vh] w-full max-w-3xl flex-col overflow-hidden rounded-xl border bg-white shadow-2xl">
        <header className="flex items-center justify-between border-b px-4 py-3">
          <div>
            <div className="flex items-center gap-2 text-sm font-semibold">
              <Package className="h-4 w-4" /> Instalar time de agentes
            </div>
            <p className="text-xs text-muted-foreground">
              projeto: <span className="font-mono">{projectRoot || '…'}</span>
            </p>
          </div>
          <Button size="icon" variant="ghost" className="h-7 w-7" onClick={onClose}>
            <X className="h-4 w-4" />
          </Button>
        </header>

        <div className="soft-scroll flex-1 overflow-y-auto px-4 py-3">
          {loadErr && (
            <div className="mb-3 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-700">
              {loadErr}
            </div>
          )}

          {!categories && !loadErr && (
            <p className="py-10 text-center text-xs text-muted-foreground">carregando…</p>
          )}

          {categories && categories.length === 0 && (
            <p className="py-10 text-center text-xs text-muted-foreground">
              Nenhum pack disponível.
            </p>
          )}

          {categories?.map((cat) => (
            <section key={cat.name} className="mb-5">
              <h3 className="text-xs font-semibold uppercase tracking-wide text-slate-500">
                {cat.label}
              </h3>
              {cat.description && (
                <p className="text-xs text-muted-foreground">{cat.description}</p>
              )}

              <div className="mt-2 space-y-2">
                {cat.packs.map((p) => {
                  const ref = `${cat.name}/${p.name}`;
                  const expanded = expandedPack === ref;
                  const sel = getPicked(ref, p.agents);
                  const all = sel.size === p.agents.length;
                  return (
                    <div key={ref} className="rounded-md border">
                      <button
                        type="button"
                        onClick={() => setExpandedPack(expanded ? null : ref)}
                        className="flex w-full items-center justify-between gap-2 px-3 py-2 text-left hover:bg-slate-50"
                      >
                        <div className="flex items-center gap-2">
                          {expanded ? (
                            <ChevronDown className="h-3.5 w-3.5 text-slate-400" />
                          ) : (
                            <ChevronRight className="h-3.5 w-3.5 text-slate-400" />
                          )}
                          <span className="text-sm font-semibold">{p.label}</span>
                          <Badge variant="outline">{p.agents.length} agentes</Badge>
                        </div>
                        <span className="font-mono text-xs text-slate-400">{ref}</span>
                      </button>

                      {expanded && (
                        <div className="border-t px-3 py-3">
                          {p.description && (
                            <p className="mb-3 text-xs text-slate-600">{p.description}</p>
                          )}

                          <div className="mb-3 flex items-center justify-between">
                            <span className="text-xs font-semibold text-slate-500">
                              Selecionados: {sel.size}/{p.agents.length}
                            </span>
                            <button
                              type="button"
                              className="text-xs text-slate-500 hover:text-slate-900"
                              onClick={() => resetPicked(ref, p.agents)}
                            >
                              todos
                            </button>
                          </div>

                          <div className="grid grid-cols-2 gap-1 sm:grid-cols-3">
                            {p.agents.map((a) => {
                              const on = sel.has(a);
                              return (
                                <label
                                  key={a}
                                  className={`flex cursor-pointer items-center gap-1.5 rounded border px-2 py-1 text-xs ${
                                    on
                                      ? 'border-primary/40 bg-primary/5 text-slate-900'
                                      : 'border-slate-200 bg-slate-50 text-slate-500'
                                  }`}
                                >
                                  <input
                                    type="checkbox"
                                    checked={on}
                                    onChange={() => toggleAgent(ref, a, p.agents)}
                                    className="h-3 w-3"
                                  />
                                  <span className="truncate">{a}</span>
                                </label>
                              );
                            })}
                          </div>

                          <div className="mt-4 flex items-center justify-between gap-2">
                            <label className="flex items-center gap-1.5 text-xs text-slate-600">
                              <input
                                type="checkbox"
                                checked={overwrite}
                                onChange={(e) => setOverwrite(e.target.checked)}
                                className="h-3 w-3"
                              />
                              sobrescrever existentes
                            </label>
                            <Button
                              size="sm"
                              disabled={sel.size === 0 || applying === ref}
                              onClick={() => install(ref, p.agents)}
                              className="gap-1.5"
                            >
                              {applying === ref ? 'Instalando…' : (
                                <>
                                  <Check className="h-3.5 w-3.5" />
                                  Instalar {all ? 'todos' : `${sel.size}`}
                                </>
                              )}
                            </Button>
                          </div>
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            </section>
          ))}

          {applyErr && (
            <div className="mt-3 flex items-start gap-2 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-700">
              <AlertCircle className="mt-0.5 h-3.5 w-3.5 shrink-0" /> {applyErr}
            </div>
          )}

          {result && (
            <div className="mt-3 rounded-md border border-emerald-200 bg-emerald-50 px-3 py-2 text-xs text-emerald-800">
              <div className="flex items-center gap-1.5 font-semibold">
                <Check className="h-3.5 w-3.5" /> {result.ref} instalado — {summary}
              </div>
              {result.r.added.length > 0 && (
                <div className="mt-1">+ {result.r.added.join(', ')}</div>
              )}
              {result.r.replaced.length > 0 && (
                <div className="mt-1">↻ {result.r.replaced.join(', ')}</div>
              )}
              {result.r.skipped.length > 0 && (
                <div className="mt-1 text-emerald-700/80">
                  = preservados (use "sobrescrever"): {result.r.skipped.join(', ')}
                </div>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
