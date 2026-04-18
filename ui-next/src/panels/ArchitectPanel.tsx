import { useState } from 'react';
import { X, Sparkles, MessageSquare, Check } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { ensureArchitectInstalled } from '@/lib/architect';

interface Props {
  onClose: () => void;
  onOpenChat: (agent: string) => void;
}

export function ArchitectPanel({ onClose, onOpenChat }: Props) {
  const [installing, setInstalling] = useState(false);
  const [installed, setInstalled] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const install = async () => {
    setErr(null);
    setInstalling(true);
    try {
      await ensureArchitectInstalled();
      setInstalled(true);
    } catch (e: any) {
      setErr(e?.message || 'falha');
    } finally {
      setInstalling(false);
    }
  };

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-slate-900/50 backdrop-blur-sm p-4">
      <div className="flex max-h-[85vh] w-full max-w-xl flex-col overflow-hidden rounded-xl border bg-white shadow-2xl">
        <header className="flex items-center justify-between border-b px-4 py-3">
          <div>
            <div className="flex items-center gap-2 text-sm font-semibold">
              <Sparkles className="h-4 w-4" /> Arquiteto
            </div>
            <p className="text-xs text-muted-foreground">
              agente meta que entrevista o humano e desenha um time customizado
            </p>
          </div>
          <Button size="icon" variant="ghost" className="h-7 w-7" onClick={onClose}>
            <X className="h-4 w-4" />
          </Button>
        </header>

        <div className="soft-scroll flex-1 overflow-y-auto p-5 text-sm">
          <p className="mb-3">
            O <strong>Arquiteto</strong> é um agente meta que conversa com você pra entender o
            negócio e gerar um <strong>seed customizado</strong> (pack de agentes) adequado.
          </p>
          <p className="mb-3 text-xs text-slate-600">
            Diferente de wizard tradicional: é chat normal no canvas, com o mesmo ChatPanel
            dos outros agentes. No final da entrevista você diz “gera o seed” e ele grava em{' '}
            <code className="rounded bg-slate-100 px-1">plugin/seeds/custom/&lt;slug&gt;/</code>.
          </p>

          <ol className="mb-4 list-inside list-decimal space-y-1 text-xs text-slate-700">
            <li>Clique em <strong>instalar arquiteto</strong> — adiciona o sprite ao canvas.</li>
            <li>Clique em <strong>abrir chat</strong> — entrevista começa.</li>
            <li>Responda naturalmente, em texto livre.</li>
            <li>Diga “gera o seed” quando o time estiver claro.</li>
          </ol>

          {err && (
            <div className="mb-3 rounded border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-700">{err}</div>
          )}

          <div className="flex justify-end gap-2">
            {!installed && (
              <Button onClick={install} disabled={installing} size="sm" className="gap-1.5">
                <Sparkles className="h-3.5 w-3.5" />
                {installing ? 'instalando…' : 'instalar arquiteto'}
              </Button>
            )}
            {installed && (
              <>
                <span className="mr-auto inline-flex items-center gap-1 text-xs text-emerald-700">
                  <Check className="h-3.5 w-3.5" /> instalado
                </span>
                <Button
                  onClick={() => { onOpenChat('architect'); onClose(); }}
                  size="sm"
                  className="gap-1.5"
                >
                  <MessageSquare className="h-3.5 w-3.5" /> abrir chat
                </Button>
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
