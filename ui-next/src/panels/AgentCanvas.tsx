import { useEffect, useRef } from 'react';
import { AgentCanvas as PixiCanvas } from '@/game/agentsPixi';
import type { AgentsData } from '@/lib/types';

interface Props {
  agents: AgentsData | null;
  selected: string | null;
  onSelect: (name: string) => void;
  workingAgent?: string | null;
}

export function AgentCanvas({ agents, selected, onSelect, workingAgent }: Props) {
  const elRef = useRef<HTMLDivElement>(null);
  const pixiRef = useRef<PixiCanvas | null>(null);

  useEffect(() => {
    if (!elRef.current) return;
    const pixi = new PixiCanvas({ onSelect });
    pixiRef.current = pixi;
    pixi.mount(elRef.current);
    return () => {
      pixi.destroy();
      pixiRef.current = null;
    };
  }, []);

  useEffect(() => {
    if (pixiRef.current && agents) pixiRef.current.update(agents);
  }, [agents]);

  useEffect(() => {
    pixiRef.current?.setSelected(selected);
  }, [selected]);

  useEffect(() => {
    pixiRef.current?.setWorking(workingAgent || null);
  }, [workingAgent]);

  return (
    <div className="relative h-full w-full overflow-hidden rounded-lg border bg-slate-50">
      <div ref={elRef} className="h-full w-full" />
      <div className="pointer-events-none absolute left-3 top-3 flex gap-1.5 text-[10px] font-medium uppercase tracking-wide">
        <Legend color="#6366f1" label="core" />
        <Legend color="#0ea5e9" label="specialist" />
        <Legend color="#a855f7" label="integrator" />
        <Legend color="#f97316" label="motor" />
        <Legend color="#ef4444" label="firefighter" />
      </div>
    </div>
  );
}

function Legend({ color, label }: { color: string; label: string }) {
  return (
    <div className="flex items-center gap-1 rounded-full bg-white/80 px-2 py-1 shadow-sm backdrop-blur">
      <span
        className="inline-block h-2 w-2 rounded-full"
        style={{ background: color }}
        aria-hidden
      />
      <span className="text-slate-600">{label}</span>
    </div>
  );
}
