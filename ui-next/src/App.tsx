import { useMemo, useState } from 'react';
import { Brain, LogOut, Users, Package, Workflow, BookOpen, Code2, Sparkles, Sun, Moon } from 'lucide-react';
import { useData } from '@/hooks/useData';
import { useCurrentHuman } from '@/hooks/useCurrentHuman';
import { useTheme } from '@/hooks/useTheme';
import { ModulesPanel } from '@/panels/ModulesPanel';
import { AgentCanvas } from '@/panels/AgentCanvas';
import { ChatPanel } from '@/panels/ChatPanel';
import { AgentInfoPanel } from '@/panels/AgentInfoPanel';
import { HumansPanel } from '@/panels/HumansPanel';
import { MyChatsPanel } from '@/panels/MyChatsPanel';
import { SeedsPanel } from '@/panels/SeedsPanel';
import { PipelinesPanel } from '@/panels/PipelinesPanel';
import { BestPracticesPanel } from '@/panels/BestPracticesPanel';
import { IdeExportPanel } from '@/panels/IdeExportPanel';
import { ArchitectPanel } from '@/panels/ArchitectPanel';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { HumanOnboarding } from '@/components/HumanOnboarding';

function humanInitials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (!parts.length) return '?';
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
  return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase();
}

export default function App() {
  const [selected, setSelected] = useState<string | null>(null);
  const [chatStreaming, setChatStreaming] = useState(false);
  const [humansOpen, setHumansOpen] = useState(false);
  const [humansPreselected, setHumansPreselected] = useState<string | null>(null);
  const [myChatsOpen, setMyChatsOpen] = useState(false);
  const [humansTick, setHumansTick] = useState(0);
  const [seedsOpen, setSeedsOpen] = useState(false);
  const [pipelinesOpen, setPipelinesOpen] = useState(false);
  const [bestPracticesOpen, setBestPracticesOpen] = useState(false);
  const [ideExportOpen, setIdeExportOpen] = useState(false);
  const [architectOpen, setArchitectOpen] = useState(false);
  const { data, err } = useData(10000, chatStreaming);
  const { humanId, human, setCurrent } = useCurrentHuman();
  const { theme, toggle: toggleTheme } = useTheme();

  const agents = data?.agents ?? null;
  const agentCfg = useMemo(
    () => (selected && agents ? agents.manifest.agents[selected] : null) ?? null,
    [selected, agents],
  );
  const agentContext = useMemo(
    () => (selected && agents ? agents.contexts[selected] || '' : ''),
    [selected, agents],
  );

  const showOnboarding = !humanId;

  return (
    <div className="flex h-screen w-screen flex-col bg-background text-foreground">
      {showOnboarding && (
        <HumanOnboarding
          onDone={(id) => {
            setCurrent(id);
            setHumansTick((t) => t + 1);
          }}
        />
      )}
      {humansOpen && humanId && (
        <HumansPanel
          meId={humanId}
          preselectedId={humansPreselected}
          onClose={() => {
            setHumansOpen(false);
            setHumansPreselected(null);
          }}
        />
      )}
      {seedsOpen && (
        <SeedsPanel onClose={() => setSeedsOpen(false)} />
      )}
      {pipelinesOpen && (
        <PipelinesPanel
          onClose={() => setPipelinesOpen(false)}
          meHumanId={humanId}
          meHumanName={human?.name || null}
          onOpenChat={(name) => {
            setPipelinesOpen(false);
            setSelected(name);
          }}
        />
      )}
      {bestPracticesOpen && (
        <BestPracticesPanel onClose={() => setBestPracticesOpen(false)} />
      )}
      {ideExportOpen && (
        <IdeExportPanel onClose={() => setIdeExportOpen(false)} />
      )}
      {architectOpen && (
        <ArchitectPanel
          onClose={() => setArchitectOpen(false)}
          onOpenChat={(name) => setSelected(name)}
        />
      )}
      {myChatsOpen && humanId && human && (
        <MyChatsPanel
          meId={humanId}
          meName={human.name}
          onClose={() => setMyChatsOpen(false)}
          onOpenAgent={(name) => setSelected(name)}
          onOpenHuman={(id) => {
            setHumansPreselected(id);
            setHumansOpen(true);
          }}
        />
      )}
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
          <Button
            size="icon"
            variant="outline"
            className="ml-2 h-8 w-8"
            onClick={toggleTheme}
            title={theme === 'dark' ? 'mudar para tema claro' : 'mudar para tema escuro'}
            aria-label="alternar tema"
          >
            {theme === 'dark' ? <Sun className="h-3.5 w-3.5" /> : <Moon className="h-3.5 w-3.5" />}
          </Button>
          <Button
            size="sm"
            variant="outline"
            className="gap-1.5"
            onClick={() => setArchitectOpen(true)}
            title="arquiteto: desenha um time customizado"
          >
            <Sparkles className="h-3.5 w-3.5" /> arquiteto
          </Button>
          <Button
            size="sm"
            variant="outline"
            className="gap-1.5"
            onClick={() => setSeedsOpen(true)}
            title="instalar time de agentes a partir de templates"
          >
            <Package className="h-3.5 w-3.5" /> time
          </Button>
          <Button
            size="sm"
            variant="outline"
            className="gap-1.5"
            onClick={() => setPipelinesOpen(true)}
            title="rodar fluxos reproduzíveis com checkpoints"
          >
            <Workflow className="h-3.5 w-3.5" /> pipelines
          </Button>
          <Button
            size="sm"
            variant="outline"
            className="gap-1.5"
            onClick={() => setBestPracticesOpen(true)}
            title="biblioteca de best-practices"
          >
            <BookOpen className="h-3.5 w-3.5" /> práticas
          </Button>
          <Button
            size="sm"
            variant="outline"
            className="gap-1.5"
            onClick={() => setIdeExportOpen(true)}
            title="exportar agentes pro formato de cada IDE"
          >
            <Code2 className="h-3.5 w-3.5" /> ide
          </Button>
          {human && (
            <Button
              size="sm"
              variant="outline"
              className="gap-1.5"
              onClick={() => setHumansOpen(true)}
              title="falar com outros humanos"
            >
              <Users className="h-3.5 w-3.5" /> humanos
            </Button>
          )}
          {human && (
            <div className="ml-1 flex items-center gap-1.5 rounded-full border bg-white px-1 py-1 pr-2.5 text-xs">
              <span className="flex h-6 w-6 items-center justify-center rounded-full bg-primary text-[10px] font-bold text-primary-foreground">
                {humanInitials(human.name)}
              </span>
              <span className="font-medium">{human.name}</span>
              <Button
                size="icon"
                variant="ghost"
                className="h-5 w-5"
                onClick={() => setCurrent(null)}
                aria-label="sair"
                title="trocar humano"
              >
                <LogOut className="h-3 w-3" />
              </Button>
            </div>
          )}
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
                manifest={agents?.manifest}
                logSummary={agents?.logs?.[selected]}
                onSelect={setSelected}
                onDeleted={() => setSelected(null)}
              />
            </div>
          )}
        </aside>

        <main className="relative overflow-hidden rounded-lg border bg-white shadow-sm">
          <AgentCanvas
            agents={agents}
            selected={selected}
            onSelect={(n) => setSelected((prev) => (prev === n ? null : n))}
            onHumanSelect={(id) => {
              if (id === humanId) {
                setMyChatsOpen((o) => !o);
              } else if (humansOpen && humansPreselected === id) {
                setHumansOpen(false);
                setHumansPreselected(null);
              } else {
                setHumansPreselected(id);
                setHumansOpen(true);
              }
            }}
            workingAgent={chatStreaming ? selected : null}
            meHumanId={humanId}
            humansTick={humansTick}
          />
        </main>

        <aside className="overflow-hidden rounded-lg border bg-white shadow-sm">
          <ChatPanel
            agent={selected}
            agentCfg={agentCfg}
            humanId={humanId}
            humanName={human?.name || null}
            onClose={() => setSelected(null)}
            onStreamingChange={setChatStreaming}
          />
        </aside>
      </div>
    </div>
  );
}
