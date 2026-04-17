import { useMemo, useState } from 'react';
import { Brain } from 'lucide-react';
import { useData } from '@/hooks/useData';
import { ModulesPanel } from '@/panels/ModulesPanel';
import { AgentCanvas } from '@/panels/AgentCanvas';
import { ChatPanel } from '@/panels/ChatPanel';
import { AgentInfoPanel } from '@/panels/AgentInfoPanel';
import { Badge } from '@/components/ui/badge';

export default function App() {
  const [selected, setSelected] = useState<string | null>(null);
  const [chatStreaming, setChatStreaming] = useState(false);
  const { data, err } = useData(10000, chatStreaming);

  const agents = data?.agents ?? null;
  const agentCfg = useMemo(
    () => (selected && agents ? agents.manifest.agents[selected] : null) ?? null,
    [selected, agents],
  );
  const agentContext = useMemo(
    () => (selected && agents ? agents.contexts[selected] || '' : ''),
    [selected, agents],
  );

  return (
    <div className="flex h-screen w-screen flex-col bg-slate-50 text-slate-900">
      <header className="flex items-center justify-between border-b bg-white/70 px-6 py-3 backdrop-blur">
        <div className="flex items-center gap-2">
          <div className="flex h-8 w-8 items-center justify-center rounded-md bg-primary/10">
            <Brain className="h-4 w-4 text-primary" />
          </div>
          <div>
            <h1 className="text-sm font-semibold leading-tight">project-mem</h1>
            <p className="text-[11px] text-muted-foreground leading-tight">
              {data?.project.name || '…'}
            </p>
          </div>
        </div>
        <div className="flex items-center gap-2">
          {err && <Badge variant="destructive">offline</Badge>}
          {chatStreaming && <Badge variant="success">chat ao vivo</Badge>}
          {data && <Badge variant="outline">{data.stats?.sessions || 0} sessões</Badge>}
        </div>
      </header>

      <div className="grid flex-1 grid-cols-[260px_1fr_460px] gap-3 overflow-hidden p-3">
        <aside className="soft-scroll overflow-y-auto pr-1">
          <ModulesPanel data={data} />
          {selected && agentCfg && (
            <div className="mt-3">
              <AgentInfoPanel
                name={selected}
                cfg={agentCfg}
                context={agentContext}
                count={agents?.counts[selected]}
                lastInvoked={agents?.recent[selected]}
              />
            </div>
          )}
        </aside>

        <main className="relative overflow-hidden rounded-lg border bg-white shadow-sm">
          <AgentCanvas
            agents={agents}
            selected={selected}
            onSelect={setSelected}
            workingAgent={chatStreaming ? selected : null}
            workingLabel="pensando…"
          />
        </main>

        <aside className="overflow-hidden rounded-lg border bg-white shadow-sm">
          <ChatPanel
            agent={selected}
            agentCfg={agentCfg}
            onStreamingChange={setChatStreaming}
          />
        </aside>
      </div>
    </div>
  );
}
