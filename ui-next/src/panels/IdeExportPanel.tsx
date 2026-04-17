import { useState } from 'react';
import { X, Code2, Check, Download } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { applyIde, IDE_META, type SupportedIde } from '@/lib/ides';

interface Props {
  onClose: () => void;
}

const IDES: SupportedIde[] = ['claude-code', 'cursor', 'vscode-copilot', 'codex'];

export function IdeExportPanel({ onClose }: Props) {
  const [busy, setBusy] = useState<SupportedIde | null>(null);
  const [last, setLast] = useState<{ ide: SupportedIde; written: string[] } | null>(null);
  const [err, setErr] = useState<string | null>(null);

  const exportFor = async (ide: SupportedIde) => {
    setErr(null);
    setLast(null);
    setBusy(ide);
    try {
      const r = await applyIde(ide);
      setLast({ ide, written: r.written });
    } catch (e: any) {
      setErr(e?.message || 'falha no export');
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-slate-900/50 backdrop-blur-sm p-4">
      <div className="flex max-h-[85vh] w-full max-w-2xl flex-col overflow-hidden rounded-xl border bg-white shadow-2xl">
        <header className="flex items-center justify-between border-b px-4 py-3">
          <div>
            <div className="flex items-center gap-2 text-sm font-semibold">
              <Code2 className="h-4 w-4" /> Exportar para IDE
            </div>
            <p className="text-[11px] text-muted-foreground">
              gera os agentes no formato nativo de cada editor. fonte única: .claude-project-memory/
            </p>
          </div>
          <Button size="icon" variant="ghost" className="h-7 w-7" onClick={onClose}>
            <X className="h-4 w-4" />
          </Button>
        </header>

        <div className="soft-scroll flex-1 overflow-y-auto p-4">
          {err && (
            <div className="mb-3 rounded border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-700">{err}</div>
          )}

          <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
            {IDES.map((ide) => {
              const meta = IDE_META[ide];
              return (
                <div key={ide} className="rounded-md border p-3">
                  <div className="flex items-start justify-between gap-2">
                    <div>
                      <p className="text-sm font-semibold">{meta.label}</p>
                      <p className="font-mono text-[10px] text-slate-500">{meta.hint}</p>
                    </div>
                    <Button
                      size="sm"
                      className="gap-1.5"
                      onClick={() => exportFor(ide)}
                      disabled={busy === ide}
                    >
                      <Download className="h-3.5 w-3.5" />
                      {busy === ide ? 'gerando…' : 'exportar'}
                    </Button>
                  </div>
                </div>
              );
            })}
          </div>

          {last && (
            <div className="mt-4 rounded-md border border-emerald-200 bg-emerald-50 px-3 py-2 text-xs text-emerald-900">
              <div className="flex items-center gap-1.5 font-semibold">
                <Check className="h-3.5 w-3.5" /> {IDE_META[last.ide].label} — {last.written.length} arquivos
              </div>
              <ul className="mt-1 space-y-0.5 font-mono text-[10px] text-emerald-800">
                {last.written.slice(0, 12).map((f) => (
                  <li key={f}>· {f}</li>
                ))}
                {last.written.length > 12 && (
                  <li>… e mais {last.written.length - 12}</li>
                )}
              </ul>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
