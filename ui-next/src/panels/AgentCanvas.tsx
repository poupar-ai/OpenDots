import { useEffect, useRef, useState } from 'react';
import { AgentCanvas as PixiCanvas } from '@/game/agentsPixi';
import { Paper, Text } from '@/ui';
import type { AgentsData } from '@/lib/types';
import { listHumans, type HumanProfile } from '@/lib/humans';

/** Controle imperativo exposto pro App (botões flutuantes de pan vertical). */
export type CanvasControls = { setVerticalPan: (dir: -1 | 0 | 1) => void };

interface Props {
  agents: AgentsData | null;
  selected: string | null;
  onSelect: (name: string) => void;
  onHumanSelect?: (id: string) => void;
  workingAgent?: string | null;
  meHumanId?: string | null;
  humansTick?: number;
  controlsRef?: React.MutableRefObject<CanvasControls | null>;
}

export function AgentCanvas({
  agents,
  selected,
  onSelect,
  onHumanSelect,
  workingAgent,
  meHumanId = null,
  humansTick = 0,
  controlsRef,
}: Props) {
  const elRef = useRef<HTMLDivElement>(null);
  const pixiRef = useRef<PixiCanvas | null>(null);
  const [humans, setHumans] = useState<HumanProfile[]>([]);

  useEffect(() => {
    if (!elRef.current) return;
    const pixi = new PixiCanvas({ onSelect, onHumanSelect, meHumanId });
    pixiRef.current = pixi;
    pixi.mount(elRef.current);
    if (controlsRef) {
      controlsRef.current = { setVerticalPan: (dir) => pixiRef.current?.setVerticalPan(dir) };
    }
    return () => {
      pixi.destroy();
      pixiRef.current = null;
      if (controlsRef) controlsRef.current = null;
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

  useEffect(() => {
    pixiRef.current?.setMeHumanId(meHumanId);
  }, [meHumanId]);

  useEffect(() => {
    let alive = true;
    const load = async () => {
      try {
        const hs = await listHumans();
        if (!alive) return;
        setHumans(hs);
        pixiRef.current?.updateHumans(hs);
      } catch {}
    };
    void load();
    const t = setInterval(load, 15_000);
    return () => { alive = false; clearInterval(t); };
  }, [humansTick]);

  useEffect(() => {
    pixiRef.current?.updateHumans(humans);
  }, [humans]);

  return (
    <div className="relative h-full w-full overflow-hidden rounded-lg border bg-slate-50">
      <div ref={elRef} className="h-full w-full" />
      <div className="pointer-events-none absolute left-3 top-3 flex flex-wrap gap-1.5">
        <Legend color="#6366f1" label="core" />
        <Legend color="#0ea5e9" label="specialist" />
        <Legend color="#a855f7" label="integrator" />
        <Legend color="#f97316" label="motor" />
        <Legend color="#ef4444" label="firefighter" />
      </div>
      <div className="pointer-events-none absolute bottom-3 right-3">
        <Paper px={8} py={4} radius="md" shadow="xs">
          <Text size="xs" muted>arrastar · rolar ↑ ↓</Text>
        </Paper>
      </div>
    </div>
  );
}

function Legend({ color, label }: { color: string; label: string }) {
  return (
    <Paper px={8} py={2} radius="md" shadow="xs">
      <span className="flex items-center gap-1">
        <span className="inline-block h-2 w-2 rounded-full" style={{ background: color }} aria-hidden />
        <Text size="xs" muted style={{ textTransform: 'uppercase', letterSpacing: '0.04em' }}>
          {label}
        </Text>
      </span>
    </Paper>
  );
}
