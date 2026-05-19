import { useEffect, useMemo, useRef, useState } from 'react';
import { Brain, LogOut, Users, Package, Workflow, BookOpen, Code2, Sparkles, Sun, Moon, ShieldAlert, KeyRound, Network, Activity, Clock } from 'lucide-react';
import { useData } from '@/hooks/useData';
import { useCurrentHuman } from '@/hooks/useCurrentHuman';
import { updateHuman } from '@/lib/humans';
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
import { ProtectionsPanel } from '@/panels/ProtectionsPanel';
import { VaultPanel } from '@/panels/VaultPanel';
import { NetworkPanel } from '@/panels/NetworkPanel';
import { SubagentPanel } from '@/panels/SubagentPanel';
import { CronPanel } from '@/panels/CronPanel';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { HumanOnboarding } from '@/components/HumanOnboarding';
import { VaultBootstrap } from '@/components/VaultBootstrap';

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
  const [protectionsOpen, setProtectionsOpen] = useState(false);
  const [vaultOpen, setVaultOpen] = useState(false);
  const [networkOpen, setNetworkOpen] = useState(false);
  const [subagentsOpen, setSubagentsOpen] = useState(false);
  const [cronOpen, setCronOpen] = useState(false);
  const [chatCollapsed, setChatCollapsed] = useState(false);
  const [prefCollapsed, setPrefCollapsed] = useState(false);
  const { data, err, refresh } = useData(10000, chatStreaming);
  const { humanId, human, setCurrent } = useCurrentHuman();
  const { theme, toggle: toggleTheme } = useTheme();

  const syncedHumanRef = useRef<string | null>(null);
  useEffect(() => {
    if (!human) {
      syncedHumanRef.current = null;
      return;
    }
    if (syncedHumanRef.current === human.id) return;
    syncedHumanRef.current = human.id;
    const pref = !!human.ui_prefs?.chatCollapsed;
    setPrefCollapsed(pref);
    // Se há agente já selecionado quando o humano logou, mantém expandido.
    setChatCollapsed(selected ? false : pref);
  }, [human, selected]);

  // Auto-expande o chat quando um agente é selecionado; ao fechar, volta à
  // preferência do humano (posição inicial).
  const prevSelectedRef = useRef<string | null>(null);
  useEffect(() => {
    const prev = prevSelectedRef.current;
    prevSelectedRef.current = selected;
    if (!prev && selected) {
      setChatCollapsed(false);
    } else if (prev && !selected) {
      setChatCollapsed(prefCollapsed);
    }
  }, [selected, prefCollapsed]);

  const toggleChatCollapsed = () => {
    setChatCollapsed((prev) => {
      const next = !prev;
      setPrefCollapsed(next);
      if (humanId) {
        void updateHuman(humanId, {
          ui_prefs: { ...(human?.ui_prefs || {}), chatCollapsed: next },
        }).catch(() => {});
      }
      return next;
    });
  };

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
      <VaultBootstrap onOpenVault={() => setVaultOpen(true)} />
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
        <SeedsPanel
          onClose={() => setSeedsOpen(false)}
          onInstalled={refresh}
        />
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
      {protectionsOpen && (
        <ProtectionsPanel onClose={() => setProtectionsOpen(false)} />
      )}
      {vaultOpen && (
        <VaultPanel onClose={() => setVaultOpen(false)} />
      )}
      {subagentsOpen && (
        <SubagentPanel onClose={() => setSubagentsOpen(false)} />
      )}
      {cronOpen && (
        <CronPanel onClose={() => setCronOpen(false)} agents={agents} />
      )}
      {networkOpen && (
        <NetworkPanel onClose={() => setNetworkOpen(false)} />
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
      <header className="flex items-center justify-between border-b bg-white/70 px-6 py-3.5 backdrop-blur">
        <div className="flex items-center gap-2.5">
          <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-primary/10">
            <Brain className="h-5 w-5 text-primary" />
          </div>
          <div>
            <h1 className="text-base font-semibold leading-tight">project-mem</h1>
            <p className="text-xs text-muted-foreground leading-tight">
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
            className="ml-2"
            onClick={toggleTheme}
            title={theme === 'dark' ? 'mudar para tema claro' : 'mudar para tema escuro'}
            aria-label="alternar tema"
          >
            {theme === 'dark' ? <Sun className="h-4 w-4" /> : <Moon className="h-4 w-4" />}
          </Button>
          <Button
            variant="outline"
            className="gap-2"
            onClick={() => setArchitectOpen(true)}
            title="arquiteto: desenha um time customizado"
          >
            <Sparkles className="h-4 w-4" /> arquiteto
          </Button>
          <Button
            variant="outline"
            className="gap-2"
            onClick={() => setSeedsOpen(true)}
            title="instalar time de agentes a partir de templates"
          >
            <Package className="h-4 w-4" /> time
          </Button>
          <Button
            variant="outline"
            className="gap-2"
            onClick={() => setPipelinesOpen(true)}
            title="rodar fluxos reproduzíveis com checkpoints"
          >
            <Workflow className="h-4 w-4" /> pipelines
          </Button>
          <Button
            variant="outline"
            className="gap-2"
            onClick={() => setBestPracticesOpen(true)}
            title="biblioteca de best-practices"
          >
            <BookOpen className="h-4 w-4" /> práticas
          </Button>
          <Button
            variant="outline"
            className="gap-2"
            onClick={() => setIdeExportOpen(true)}
            title="exportar agentes pro formato de cada IDE"
          >
            <Code2 className="h-4 w-4" /> ide
          </Button>
          <Button
            variant="outline"
            className="gap-2"
            onClick={() => setProtectionsOpen(true)}
            title="proteções — motor de intent destrutivo"
          >
            <ShieldAlert className="h-4 w-4 text-red-600" /> proteções
          </Button>
          <Button
            variant="outline"
            className="gap-2"
            onClick={() => setVaultOpen(true)}
            title="cofre — secrets/envs encriptados"
          >
            <KeyRound className="h-4 w-4 text-amber-600" /> cofre
          </Button>
          <Button
            variant="outline"
            className="gap-2"
            onClick={() => setNetworkOpen(true)}
            title="rede — bind do servidor (localhost/LAN/IP)"
          >
            <Network className="h-4 w-4 text-sky-600" /> rede
          </Button>
          <Button
            variant="outline"
            className="gap-2"
            onClick={() => setSubagentsOpen(true)}
            title="subagents — workstreams paralelos disparados por agents"
          >
            <Activity className="h-4 w-4 text-indigo-600" /> subagents
          </Button>
          <Button
            variant="outline"
            className="gap-2"
            onClick={() => setCronOpen(true)}
            title="agendamentos — jobs recorrentes via cron"
          >
            <Clock className="h-4 w-4 text-emerald-600" /> cron
          </Button>
          {human && (
            <Button
              variant="outline"
              className="gap-2"
              onClick={() => setHumansOpen(true)}
              title="falar com outros humanos"
            >
              <Users className="h-4 w-4" /> humanos
            </Button>
          )}
          {human && (
            <div className="ml-1 flex items-center gap-2 rounded-full border bg-white py-1 pl-1 pr-3 text-sm">
              <span className="flex h-7 w-7 items-center justify-center rounded-full bg-primary text-xs font-bold text-primary-foreground">
                {humanInitials(human.name)}
              </span>
              <span className="font-medium">{human.name}</span>
              <Button
                size="icon"
                variant="ghost"
                className="h-6 w-6"
                onClick={() => setCurrent(null)}
                aria-label="sair"
                title="trocar humano"
              >
                <LogOut className="h-3.5 w-3.5" />
              </Button>
            </div>
          )}
        </div>
      </header>

      <div
        className={`grid flex-1 gap-3 overflow-hidden p-3 ${
          chatCollapsed
            ? 'grid-cols-[300px_1fr_48px]'
            : 'grid-cols-[300px_1fr_460px]'
        }`}
      >
        <aside className="soft-scroll overflow-y-auto pr-1">
          {selected && agentCfg ? (
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
          ) : (
            <ModulesPanel data={data} />
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
            allAgents={agents?.manifest.agents ?? null}
            humanId={humanId}
            humanName={human?.name || null}
            onClose={() => setSelected(null)}
            onStreamingChange={setChatStreaming}
            collapsed={chatCollapsed}
            onToggleCollapse={toggleChatCollapsed}
          />
        </aside>
      </div>
    </div>
  );
}
