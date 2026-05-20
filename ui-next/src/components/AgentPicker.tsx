import { AgentFace } from '@/components/AgentFace';
import type { AgentCfg } from '@/lib/types';

interface Props {
  agents: Record<string, AgentCfg> | null;
  onSelect: (name: string) => void;
}

/**
 * Lista tocável de agentes — usada quando não dá pra selecionar pelo canvas
 * (ex.: chat em tela cheia no celular). Reaproveitável em qualquer contexto
 * que precise escolher um agente.
 */
export function AgentPicker({ agents, onSelect }: Props) {
  const entries = agents ? Object.entries(agents) : [];

  if (entries.length === 0) {
    return (
      <div className="flex h-full items-center justify-center p-6 text-center text-sm text-muted-foreground">
        Nenhum agente instalado ainda.
      </div>
    );
  }

  return (
    <div className="h-full overflow-y-auto p-3">
      <div className="flex flex-col gap-2">
        {entries.map(([name, cfg]) => (
          <button
            key={name}
            type="button"
            onClick={() => onSelect(name)}
            className="flex items-center gap-3 rounded-xl border bg-white p-3 text-left shadow-sm hover:bg-slate-50 active:scale-[0.99]"
          >
            <AgentFace type={cfg.type} profession={(cfg as any).profession} size={38} />
            <div className="min-w-0">
              <div className="truncate text-sm font-semibold">{name}</div>
              <div className="truncate text-xs text-muted-foreground">
                {cfg.role || (cfg as any).profession || cfg.type || 'agente'}
              </div>
            </div>
          </button>
        ))}
      </div>
    </div>
  );
}
