import { lazy, Suspense, useEffect, useMemo, useRef, useState } from 'react';
import { Brain, LogOut, Users, Package, Workflow, BookOpen, Code2, Sparkles, Sun, Moon, ShieldAlert, KeyRound, Network, Activity, Clock, MessageCircle, X, PanelLeft, Plug } from 'lucide-react';
import { useData } from '@/hooks/useData';
import { useMediaQuery } from '@/hooks/useMediaQuery';
import { useCurrentHuman } from '@/hooks/useCurrentHuman';
import { updateHuman } from '@/lib/humans';
import { useTheme } from '@/hooks/useTheme';
import { CHARACTERS } from '@/lib/characters';
import { ModulesPanel } from '@/panels/ModulesPanel';
import type { CanvasControls } from '@/panels/AgentCanvas';
// Lazy: o AgentCanvas puxa o Pixi/WebGL (chunk pesado). Carrega depois do
// shell pra não travar o load inicial (sobretudo no mobile).
const AgentCanvas = lazy(() =>
  import('@/panels/AgentCanvas').then((m) => ({ default: m.AgentCanvas })),
);
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
import { McpRegistryPanel } from '@/panels/McpRegistryPanel';
import { ActionIcon, Badge, Box, Drawer, Group, colorVar } from '@/ui';
import { MainMenu, type MenuItem } from '@/components/MainMenu';
import { AgentPicker } from '@/components/AgentPicker';
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
  const [vaultReady, setVaultReady] = useState(false);
  const [networkOpen, setNetworkOpen] = useState(false);
  const [subagentsOpen, setSubagentsOpen] = useState(false);
  const [cronOpen, setCronOpen] = useState(false);
  const [mcpsOpen, setMcpsOpen] = useState(false);
  const [chatCollapsed, setChatCollapsed] = useState(false);
  const [prefCollapsed, setPrefCollapsed] = useState(false);
  const [mobileChatOpen, setMobileChatOpen] = useState(false);
  const [panelOpen, setPanelOpen] = useState(false);
  const isMobile = useMediaQuery('(max-width: 768px)');
  const canvasControls = useRef<CanvasControls | null>(null);
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

  // No celular não há painel de chat fixo — selecionar um agente (canvas ou
  // picker) abre o chat em tela cheia automaticamente.
  useEffect(() => {
    if (isMobile && selected) setMobileChatOpen(true);
  }, [isMobile, selected]);

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

  // Menu principal — data-driven. Pra adicionar uma entrada nova, basta
  // mais um objeto aqui: aparece inline no desktop e no Drawer no mobile.
  const menuItems = useMemo<MenuItem[]>(() => {
    const items: MenuItem[] = [
      { id: 'arquiteto', label: 'arquiteto', title: 'arquiteto: desenha um time customizado', icon: <Sparkles className="h-4 w-4" />, onClick: () => setArchitectOpen(true) },
      { id: 'time', label: 'time', title: 'instalar time de agentes a partir de templates', icon: <Package className="h-4 w-4" />, onClick: () => setSeedsOpen(true) },
      { id: 'pipelines', label: 'pipelines', title: 'rodar fluxos reproduzíveis com checkpoints', icon: <Workflow className="h-4 w-4" />, onClick: () => setPipelinesOpen(true) },
      { id: 'praticas', label: 'práticas', title: 'biblioteca de best-practices', icon: <BookOpen className="h-4 w-4" />, onClick: () => setBestPracticesOpen(true) },
      { id: 'ide', label: 'ide', title: 'exportar agentes pro formato de cada IDE', icon: <Code2 className="h-4 w-4" />, onClick: () => setIdeExportOpen(true) },
      { id: 'protecoes', label: 'proteções', title: 'proteções — motor de intent destrutivo', icon: <ShieldAlert className="h-4 w-4" />, iconColor: 'danger', onClick: () => setProtectionsOpen(true) },
      { id: 'cofre', label: 'cofre', title: 'cofre — secrets/envs encriptados', icon: <KeyRound className="h-4 w-4" />, iconColor: 'warning', onClick: () => setVaultOpen(true) },
      { id: 'rede', label: 'rede', title: 'rede — bind do servidor (localhost/LAN/IP)', icon: <Network className="h-4 w-4" />, iconColor: 'info', onClick: () => setNetworkOpen(true) },
      { id: 'subagents', label: 'subagents', title: 'subagents — workstreams paralelos disparados por agents', icon: <Activity className="h-4 w-4" />, iconColor: 'primary', onClick: () => setSubagentsOpen(true) },
      { id: 'cron', label: 'cron', title: 'agendamentos — jobs recorrentes via cron', icon: <Clock className="h-4 w-4" />, iconColor: 'success', onClick: () => setCronOpen(true) },
      { id: 'mcps', label: 'mcps', title: 'servidores MCP — cadastrar/compartilhar (conecta via .mcp.json)', icon: <Plug className="h-4 w-4" />, iconColor: 'info', onClick: () => setMcpsOpen(true) },
    ];
    if (human) {
      items.push({ id: 'humanos', label: 'humanos', title: 'falar com outros humanos', icon: <Users className="h-4 w-4" />, onClick: () => setHumansOpen(true) });
    }
    return items;
  }, [human]);

  // Cofre é obrigatório ANTES do humano (a senha do humano vira hash no
  // cofre). Onboarding só aparece com o cofre pronto.
  const showOnboarding = !humanId && vaultReady;

  return (
    <div className="flex h-screen w-screen flex-col bg-background text-foreground">
      <VaultBootstrap onReady={setVaultReady} />
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
      {mcpsOpen && <McpRegistryPanel onClose={() => setMcpsOpen(false)} agentNames={Object.keys(agents?.manifest.agents ?? {})} />}
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
          <div className="flex shrink-0 items-end -space-x-1.5">
            {CHARACTERS.map((c) => (
              <img key={c.id} src={c.url} alt={c.label} title={c.label} className="h-8 w-auto drop-shadow-sm" />
            ))}
          </div>
          <div className="shrink-0">
            <h1 className="text-base font-semibold leading-tight whitespace-nowrap">OpenDots</h1>
            <p className="text-xs text-muted-foreground leading-tight">
              {data?.project.name || '…'}
            </p>
          </div>
        </div>
        <Group gap="sm" wrap="nowrap">
          {/* Badges de status — escondem em telas estreitas pra dar espaço. */}
          <Box visibleFrom="sm">
            <Group gap="xs" wrap="nowrap">
              {err && <Badge color="danger" variant="filled">offline</Badge>}
              {chatStreaming && <Badge color="success" variant="filled">chat ao vivo</Badge>}
              {data && <Badge color="gray" variant="outline">{data.stats?.sessions || 0} sessões</Badge>}
            </Group>
          </Box>

          {/* Mobile: acesso ao painel lateral (Módulos / Info do agente). */}
          {isMobile && (
            <ActionIcon
              variant="default"
              color="gray"
              onClick={() => setPanelOpen(true)}
              label="abrir painel de módulos e info"
            >
              <PanelLeft className="h-4 w-4" />
            </ActionIcon>
          )}

          <ActionIcon
            variant="default"
            color="gray"
            onClick={toggleTheme}
            label={theme === 'dark' ? 'mudar para tema claro' : 'mudar para tema escuro'}
          >
            {theme === 'dark' ? <Sun className="h-4 w-4" /> : <Moon className="h-4 w-4" />}
          </ActionIcon>

          <MainMenu items={menuItems} />

          {human && (
            <Group gap="xs" wrap="nowrap">
              <span
                className="flex h-7 w-7 items-center justify-center rounded-full text-xs font-bold"
                style={{ background: colorVar('primary', 6), color: 'white' }}
              >
                {humanInitials(human.name)}
              </span>
              <Box visibleFrom="sm">
                <span className="text-sm font-medium">{human.name}</span>
              </Box>
              <ActionIcon
                variant="subtle"
                color="gray"
                onClick={() => setCurrent(null)}
                label="trocar humano"
              >
                <LogOut className="h-3.5 w-3.5" />
              </ActionIcon>
            </Group>
          )}
        </Group>
      </header>

      <div
        className={`grid flex-1 gap-3 overflow-hidden p-3 ${
          isMobile
            ? 'grid-cols-1'
            : chatCollapsed
              ? 'grid-cols-[300px_1fr_48px]'
              : 'grid-cols-[300px_1fr_460px]'
        }`}
      >
        {!isMobile && (
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
        )}

        <main className="relative overflow-hidden rounded-lg border bg-white shadow-sm">
          <Suspense
            fallback={
              <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
                carregando o canvas…
              </div>
            }
          >
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
              controlsRef={canvasControls}
            />
          </Suspense>
        </main>

        {!isMobile && (
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
        )}
      </div>

      {/* Mobile: painel lateral (Módulos / Info do agente) via drawer. */}
      {isMobile && (
        <Drawer
          opened={panelOpen}
          onClose={() => setPanelOpen(false)}
          title={selected && agentCfg ? selected : 'Módulos'}
          position="left"
          size="sm"
        >
          {selected && agentCfg ? (
            <AgentInfoPanel
              name={selected}
              cfg={agentCfg}
              context={agentContext}
              count={agents?.counts[selected]}
              lastInvoked={agents?.recent[selected]}
              manifest={agents?.manifest}
              logSummary={agents?.logs?.[selected]}
              onSelect={(n) => {
                setSelected(n);
                setPanelOpen(false);
              }}
              onDeleted={() => {
                setSelected(null);
                setPanelOpen(false);
              }}
            />
          ) : (
            <ModulesPanel data={data} />
          )}
        </Drawer>
      )}

      {/* Mobile: controles ▲▼ (rolam o prédio) + balão flutuante do chat. */}
      {isMobile && !mobileChatOpen && (
        <div className="fixed bottom-5 right-5 z-40 flex items-center gap-3">
          <div className="flex gap-2">
            <PanFab dir={-1} controls={canvasControls} label="rolar para cima" />
            <PanFab dir={1} controls={canvasControls} label="rolar para baixo" />
          </div>
          <button
            type="button"
            onClick={() => setMobileChatOpen(true)}
            aria-label="abrir chat"
            className="relative flex h-14 w-14 items-center justify-center rounded-full shadow-lg transition active:scale-95"
            style={{ background: colorVar('primary', 6), color: 'white' }}
          >
            <MessageCircle className="h-6 w-6" />
            {chatStreaming && (
              <span className="absolute right-1 top-1 h-3 w-3 rounded-full bg-emerald-400 ring-2 ring-white" />
            )}
          </button>
        </div>
      )}
      {isMobile && mobileChatOpen && (
        <div className="fixed inset-0 z-50 flex flex-col bg-white">
          {/* Barra de fechar — sempre presente, mesmo sem agente selecionado. */}
          <div className="flex items-center gap-2 border-b bg-white px-2 py-2">
            <button
              type="button"
              onClick={() => setMobileChatOpen(false)}
              aria-label="fechar chat"
              className="flex h-9 w-9 items-center justify-center rounded-full text-slate-600 hover:bg-slate-100 active:scale-95"
            >
              <X className="h-5 w-5" />
            </button>
            <span className="flex-1 truncate text-sm font-semibold">
              {selected || 'Selecione um agente'}
            </span>
            {selected && (
              <button
                type="button"
                onClick={() => setSelected(null)}
                className="rounded-full px-3 py-1 text-xs font-medium text-primary hover:bg-primary/10"
              >
                trocar
              </button>
            )}
          </div>
          <div className="min-h-0 flex-1">
            {selected ? (
              <ChatPanel
                agent={selected}
                agentCfg={agentCfg}
                allAgents={agents?.manifest.agents ?? null}
                humanId={humanId}
                humanName={human?.name || null}
                onStreamingChange={setChatStreaming}
              />
            ) : (
              <AgentPicker
                agents={agents?.manifest.agents ?? null}
                onSelect={setSelected}
              />
            )}
          </div>
        </div>
      )}
    </div>
  );
}

/** Botão flutuante de pan vertical (segurar = rolar contínuo). Ícone em SVG. */
function PanFab({
  dir,
  controls,
  label,
}: {
  dir: -1 | 1;
  controls: React.MutableRefObject<CanvasControls | null>;
  label: string;
}) {
  const press = () => controls.current?.setVerticalPan(dir);
  const release = () => controls.current?.setVerticalPan(0);
  return (
    <button
      type="button"
      aria-label={label}
      onPointerDown={(e) => {
        e.preventDefault();
        e.currentTarget.setPointerCapture?.(e.pointerId);
        press();
      }}
      onPointerUp={release}
      onPointerLeave={release}
      onPointerCancel={release}
      className="flex h-11 w-11 select-none items-center justify-center rounded-full bg-white/90 text-slate-700 shadow-lg backdrop-blur transition active:scale-90 active:bg-white"
    >
      <svg
        viewBox="0 0 24 24"
        width="22"
        height="22"
        fill="none"
        stroke="currentColor"
        strokeWidth={2.5}
        strokeLinecap="round"
        strokeLinejoin="round"
        style={{ transform: dir === 1 ? 'rotate(180deg)' : undefined }}
        aria-hidden
      >
        <path d="M6 15l6-6 6 6" />
      </svg>
    </button>
  );
}
