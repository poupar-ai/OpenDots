import { useEffect, useState } from 'react';
import { X, BookOpen, Check, Download } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import {
  listBestPractices,
  getBestPractice,
  applyBestPractices,
  type BestPractice,
} from '@/lib/bestPractices';

interface Props {
  onClose: () => void;
}

export function BestPracticesPanel({ onClose }: Props) {
  const [items, setItems] = useState<BestPractice[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [content, setContent] = useState<string>('');
  const [installing, setInstalling] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);

  const reload = async () => {
    try {
      const j = await listBestPractices();
      setItems(j.available);
    } catch (e: any) {
      setErr(e?.message || 'falha ao listar');
    }
  };

  useEffect(() => { reload(); }, []);

  useEffect(() => {
    if (!selected) { setContent(''); return; }
    getBestPractice(selected)
      .then((j) => setContent(j.content))
      .catch((e) => setErr(e?.message || 'falha ao ler'));
  }, [selected]);

  const install = async (id: string) => {
    setInstalling(id);
    try {
      await applyBestPractices([id], false);
      await reload();
    } catch (e: any) {
      setErr(e?.message || 'falha ao instalar');
    } finally {
      setInstalling(null);
    }
  };

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-slate-900/50 backdrop-blur-sm p-4">
      <div className="flex h-[85vh] w-full max-w-5xl flex-col overflow-hidden rounded-xl border bg-white shadow-2xl">
        <header className="flex items-center justify-between border-b px-4 py-3">
          <div>
            <div className="flex items-center gap-2 text-sm font-semibold">
              <BookOpen className="h-4 w-4" /> Best-practices
            </div>
            <p className="text-[11px] text-muted-foreground">
              conhecimento de domínio injetável nos agentes, versionado por projeto
            </p>
          </div>
          <Button size="icon" variant="ghost" className="h-7 w-7" onClick={onClose}>
            <X className="h-4 w-4" />
          </Button>
        </header>

        <div className="grid flex-1 grid-cols-[280px_1fr] overflow-hidden">
          <aside className="soft-scroll overflow-y-auto border-r p-3">
            {err && (
              <div className="mb-2 rounded border border-red-200 bg-red-50 px-2 py-1.5 text-[11px] text-red-700">{err}</div>
            )}
            {!items && <p className="text-xs text-muted-foreground">carregando…</p>}
            {items?.map((p) => (
              <button
                key={p.id}
                onClick={() => setSelected(p.id)}
                className={`mb-1.5 block w-full rounded border px-2 py-2 text-left text-xs transition ${
                  selected === p.id
                    ? 'border-primary/50 bg-primary/5'
                    : 'border-slate-200 hover:bg-slate-50'
                }`}
              >
                <div className="flex items-center justify-between gap-2">
                  <span className="font-semibold">{p.name}</span>
                  {p.installed && <Check className="h-3 w-3 text-emerald-600" />}
                </div>
                <p className="mt-0.5 text-[10px] text-slate-500">{p.scope}</p>
                {p.tags && p.tags.length > 0 && (
                  <div className="mt-1 flex flex-wrap gap-1">
                    {p.tags.slice(0, 4).map((t) => (
                      <span key={t} className="rounded bg-slate-100 px-1 py-0.5 text-[9px] text-slate-600">
                        {t}
                      </span>
                    ))}
                  </div>
                )}
              </button>
            ))}
          </aside>

          <main className="soft-scroll overflow-y-auto p-4">
            {!selected && (
              <p className="py-10 text-center text-xs text-slate-500">
                Selecione uma prática na lista.
              </p>
            )}
            {selected && items && (
              (() => {
                const p = items.find(x => x.id === selected);
                if (!p) return null;
                return (
                  <div>
                    <div className="mb-3 flex items-start justify-between gap-3">
                      <div>
                        <h2 className="text-base font-semibold">{p.name}</h2>
                        <p className="text-[11px] text-slate-500">
                          {p.scope}
                          {p.tags && ` · ${p.tags.join(', ')}`}
                        </p>
                        {p.when_to_use && (
                          <p className="mt-1 whitespace-pre-line text-xs text-slate-700">
                            <strong>Quando usar:</strong> {p.when_to_use}
                          </p>
                        )}
                        {p.not_for && (
                          <p className="mt-0.5 text-xs text-slate-500">
                            <strong>Não usar para:</strong> {p.not_for}
                          </p>
                        )}
                      </div>
                      {!p.installed ? (
                        <Button
                          size="sm"
                          onClick={() => install(p.id)}
                          disabled={installing === p.id}
                          className="shrink-0 gap-1.5"
                        >
                          <Download className="h-3.5 w-3.5" />
                          {installing === p.id ? 'instalando…' : 'instalar'}
                        </Button>
                      ) : (
                        <Badge variant="outline" className="shrink-0">
                          <Check className="h-3 w-3" /> instalado
                        </Badge>
                      )}
                    </div>
                    <pre className="whitespace-pre-wrap rounded bg-slate-50 p-3 font-mono text-[11px] leading-relaxed text-slate-800">
                      {content || '(sem conteúdo)'}
                    </pre>
                  </div>
                );
              })()
            )}
          </main>
        </div>
      </div>
    </div>
  );
}
