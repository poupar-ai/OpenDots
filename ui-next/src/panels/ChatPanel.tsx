import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { Send, X, Bot, Brain, User as UserIcon, Sparkles, Loader2, PanelRightClose, PanelRightOpen } from 'lucide-react';
import { ActionIcon, Badge, Button, Loader } from '@/ui';
import type { Color } from '@/ui';
import { CopyButton } from '@/components/CopyButton';
import { AgentFace } from '@/components/AgentFace';
import { MessageBubble, DayDivider } from '@/components/chat';
import { cn, fmtDayKey, fmtDayLabel } from '@/lib/utils';
import type { AgentCfg } from '@/lib/types';
import { useChat } from '@/hooks/useChat';
import { listHumans, type HumanProfile } from '@/lib/humans';

interface Props {
  agent: string | null;
  agentCfg: AgentCfg | null;
  allAgents?: Record<string, AgentCfg> | null;
  humanId?: string | null;
  humanName?: string | null;
  onClose?: () => void;
  onStreamingChange?: (streaming: boolean) => void;
  collapsed?: boolean;
  onToggleCollapse?: () => void;
}

type MentionTarget = {
  id: string;        // nome a inserir no texto
  display: string;   // nome pra mostrar na lista
  kind: 'agent' | 'human';
  subtitle?: string;
  emoji?: string;
};

// Detecta trigger de @ antes do caret: retorna posição do @ e a query digitada.
// Só considera @ precedido por início-de-string ou whitespace, e query sem espaços.
function findMentionTrigger(text: string, caret: number): { start: number; query: string } | null {
  for (let i = caret - 1; i >= 0; i--) {
    const ch = text[i];
    if (ch === '@') {
      const prev = i > 0 ? text[i - 1] : '';
      if (prev === '' || /\s/.test(prev)) {
        return { start: i, query: text.slice(i + 1, caret) };
      }
      return null;
    }
    if (/\s/.test(ch)) return null;
  }
  return null;
}

function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (!parts.length) return '?';
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
  return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase();
}

const TYPE_BADGE: Record<string, Color> = {
  core: 'primary',
  specialist: 'gray',
  integrator: 'success',
  motor: 'warning',
  firefighter: 'danger',
};

export function ChatPanel({
  agent,
  agentCfg,
  allAgents = null,
  humanId = null,
  humanName = null,
  onClose,
  onStreamingChange,
  collapsed = false,
  onToggleCollapse,
}: Props) {
  const { messages, send, sending, err, loading, isStreaming, streamingPhase } = useChat(agent, humanId);
  const [input, setInput] = useState('');
  const scrollRef = useRef<HTMLDivElement>(null);
  const taRef = useRef<HTMLTextAreaElement>(null);

  // Lazy-load por janela: renderiza só as N mais recentes e revela lotes
  // antigos conforme o usuário rola pro topo — não sobrecarrega o DOM/markdown
  // mesmo com históricos longos. (Forward-compatible com paginação de servidor.)
  const BATCH = 30;
  const [visibleCount, setVisibleCount] = useState(BATCH);
  useEffect(() => { setVisibleCount(BATCH); }, [agent]);
  const windowStart = Math.max(0, messages.length - visibleCount);
  const hasOlder = windowStart > 0;
  const windowed = useMemo(() => messages.slice(windowStart), [messages, windowStart]);

  // Carregar-mais-antigas preservando a posição: guarda a altura antes de
  // expandir a janela e reajusta scrollTop depois, pra não "pular".
  const loadingOlderRef = useRef(false);
  const prevScrollHeightRef = useRef<number | null>(null);
  const onScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    if (el.scrollTop <= 64 && hasOlder && !loadingOlderRef.current) {
      loadingOlderRef.current = true;
      prevScrollHeightRef.current = el.scrollHeight;
      setVisibleCount((c) => Math.min(messages.length, c + BATCH));
    }
  };
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    if (loadingOlderRef.current && prevScrollHeightRef.current != null) {
      el.scrollTop = el.scrollHeight - prevScrollHeightRef.current;
      loadingOlderRef.current = false;
      prevScrollHeightRef.current = null;
    }
  }, [visibleCount]);

  // Humanos pro autocomplete de @ — carrega uma vez por montagem.
  const [humans, setHumans] = useState<HumanProfile[]>([]);
  useEffect(() => {
    let cancelled = false;
    listHumans()
      .then((hs) => { if (!cancelled) setHumans(hs); })
      .catch(() => {});
    return () => { cancelled = true; };
  }, []);

  // Alvos pra mention: agentes (exceto o atual) + humanos (exceto eu).
  const mentionTargets = useMemo<MentionTarget[]>(() => {
    const list: MentionTarget[] = [];
    if (allAgents) {
      for (const [name, cfg] of Object.entries(allAgents)) {
        if (name === agent) continue;
        list.push({
          id: name,
          display: name,
          kind: 'agent',
          subtitle: cfg.role || cfg.profession || cfg.type || 'agente',
          emoji: cfg.emoji,
        });
      }
    }
    for (const h of humans) {
      if (h.id === humanId) continue;
      list.push({
        id: h.name,
        display: h.name,
        kind: 'human',
        subtitle: h.description || 'humano',
      });
    }
    return list;
  }, [allAgents, agent, humans, humanId]);

  const [mention, setMention] = useState<{ start: number; query: string; hover: number } | null>(null);

  // ✨ Melhorar prompt — Haiku corrige ortografia/clareza sem mudar intenção.
  // Usuário aceita (substitui textarea) ou descarta. Não persiste nada.
  const [improve, setImprove] = useState<
    | { phase: 'loading' }
    | { phase: 'ready'; original: string; improved: string }
    | { phase: 'error'; message: string }
    | null
  >(null);

  const filteredMentions = useMemo(() => {
    if (!mention) return [];
    const q = mention.query.toLowerCase();
    const matches = mentionTargets.filter((t) => t.display.toLowerCase().includes(q));
    // agentes primeiro, depois humanos — e priorizar startsWith
    matches.sort((a, b) => {
      const aStart = a.display.toLowerCase().startsWith(q) ? 0 : 1;
      const bStart = b.display.toLowerCase().startsWith(q) ? 0 : 1;
      if (aStart !== bStart) return aStart - bStart;
      if (a.kind !== b.kind) return a.kind === 'agent' ? -1 : 1;
      return a.display.localeCompare(b.display);
    });
    return matches.slice(0, 8);
  }, [mention, mentionTargets]);

  const syncMention = (text: string, caret: number) => {
    const trig = findMentionTrigger(text, caret);
    if (!trig) return setMention(null);
    setMention((cur) => ({
      start: trig.start,
      query: trig.query,
      hover: cur && cur.start === trig.start ? cur.hover : 0,
    }));
  };

  const applyMention = (target: MentionTarget) => {
    if (!mention || !taRef.current) return;
    const ta = taRef.current;
    const before = input.slice(0, mention.start);
    const after = input.slice(ta.selectionEnd ?? mention.start + 1 + mention.query.length);
    const inserted = `@${target.id} `;
    const next = before + inserted + after;
    setInput(next);
    setMention(null);
    requestAnimationFrame(() => {
      if (!taRef.current) return;
      const caret = (before + inserted).length;
      taRef.current.focus();
      taRef.current.setSelectionRange(caret, caret);
    });
  };

  useEffect(() => {
    onStreamingChange?.(isStreaming);
  }, [isStreaming, onStreamingChange]);

  // Auto-scroll pro fundo só quando chega mensagem nova ou troca de agente —
  // depende de messages.length (não de `windowed`), então expandir a janela
  // de mensagens antigas não dispara o pulo pro fim.
  useEffect(() => {
    if (loadingOlderRef.current) return;
    const el = scrollRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
  }, [messages.length, agent]);

  const autoResize = (ta: HTMLTextAreaElement | null) => {
    if (!ta) return;
    ta.style.height = 'auto';
    const next = Math.min(Math.max(ta.scrollHeight, 72), 220);
    ta.style.height = next + 'px';
  };

  // autoResize roda em onInput — não duplicar via useEffect([input]) pra
  // evitar layout síncrono extra a cada tecla.
  useEffect(() => {
    autoResize(taRef.current);
  }, [agent]);

  // Agrupa msgs por dia e marca runs consecutivos com mesmo group_id como thread.
  const grouped = useMemo(() => {
    let lastKey = '';
    type Item =
      | { kind: 'day'; label: string }
      | { kind: 'msg'; msg: any; threadId: string | null; threadIndex: number; threadSize: number };
    const items: Item[] = [];
    const runStart: number[] = []; // índices em `items` onde thread corrente começa
    let currentGroup: string | null = null;
    const flushRun = () => {
      if (!runStart.length) return;
      const size = runStart.length;
      for (let j = 0; j < runStart.length; j++) {
        const it = items[runStart[j]];
        if (it.kind === 'msg') it.threadSize = size;
      }
      runStart.length = 0;
    };
    for (const m of windowed) {
      const k = fmtDayKey(m.ts);
      if (k !== lastKey) {
        flushRun();
        currentGroup = null;
        items.push({ kind: 'day', label: fmtDayLabel(k) });
        lastKey = k;
      }
      const gid: string | null = (m as any).group_id || null;
      if (gid && gid === currentGroup) {
        const idx = items.length;
        items.push({ kind: 'msg', msg: m, threadId: gid, threadIndex: runStart.length, threadSize: 0 });
        runStart.push(idx);
      } else {
        flushRun();
        currentGroup = gid;
        const idx = items.length;
        if (gid) {
          items.push({ kind: 'msg', msg: m, threadId: gid, threadIndex: 0, threadSize: 0 });
          runStart.push(idx);
        } else {
          items.push({ kind: 'msg', msg: m, threadId: null, threadIndex: 0, threadSize: 1 });
        }
      }
    }
    flushRun();
    return items;
  }, [windowed]);

  const [collapsedThreads, setCollapsedThreads] = useState<Set<string>>(new Set());
  const toggleThread = (gid: string) => {
    setCollapsedThreads((cur) => {
      const next = new Set(cur);
      if (next.has(gid)) next.delete(gid);
      else next.add(gid);
      return next;
    });
  };

  if (collapsed) {
    return (
      <div className="flex h-full flex-col items-center gap-3 bg-white py-3">
        <ActionIcon variant="subtle" color="gray" onClick={onToggleCollapse} label="expandir chat">
          <PanelRightOpen className="h-4 w-4" />
        </ActionIcon>
        {agent && agentCfg ? (
          <div className="flex flex-col items-center gap-2">
            <AgentFace
              type={agentCfg.type}
              profession={(agentCfg as any).profession}
              size={30}
              mood={isStreaming && streamingPhase === 'writing' ? 'happy' : 'neutral'}
            />
            {isStreaming && (
              <Loader2 className="h-3.5 w-3.5 animate-spin text-emerald-600" />
            )}
          </div>
        ) : (
          <Bot className="h-5 w-5 text-muted-foreground/50" />
        )}
      </div>
    );
  }

  if (!agent || !agentCfg) {
    return (
      <div className="flex h-full flex-col">
        {onToggleCollapse && (
          <div className="flex justify-end border-b bg-white p-2">
            <ActionIcon variant="subtle" color="gray" onClick={onToggleCollapse} label="minimizar chat">
              <PanelRightClose className="h-4 w-4" />
            </ActionIcon>
          </div>
        )}
        <div className="flex flex-1 flex-col items-center justify-center gap-2 p-6 text-center text-muted-foreground">
          <Bot className="h-10 w-10 opacity-40" />
          <p className="text-sm">Selecione um agente no canvas pra conversar</p>
        </div>
      </div>
    );
  }

  const type = (agentCfg.type || 'specialist') as keyof typeof TYPE_BADGE;
  const submit = () => {
    const msg = input.trim();
    if (!msg || sending) return;
    send(msg);
    setInput('');
    setImprove(null);
  };

  const runImprove = async () => {
    const text = input.trim();
    if (!text || improve?.phase === 'loading' || sending) return;
    setImprove({ phase: 'loading' });
    try {
      const r = await fetch('/api/chat/improve', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ text }),
      });
      const j = await r.json();
      if (!r.ok) {
        setImprove({ phase: 'error', message: j.error || `HTTP ${r.status}` });
        return;
      }
      if (!j.improved || j.improved === j.original) {
        setImprove({ phase: 'error', message: 'texto já está bom — nada a melhorar' });
        return;
      }
      setImprove({ phase: 'ready', original: j.original, improved: j.improved });
    } catch (e: any) {
      setImprove({ phase: 'error', message: e?.message || 'falha ao melhorar prompt' });
    }
  };

  const acceptImprove = () => {
    if (improve?.phase !== 'ready') return;
    setInput(improve.improved);
    setImprove(null);
    requestAnimationFrame(() => taRef.current?.focus());
  };

  const meInitials = humanName ? initials(humanName) : null;

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center gap-3 border-b bg-white px-4 py-3">
        {humanName && (
          <>
            <div
              className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-primary text-xs font-bold text-primary-foreground ring-2 ring-primary/20"
              title={`você — ${humanName}`}
            >
              {meInitials}
            </div>
            <span className="select-none text-lg text-slate-300">↔</span>
          </>
        )}
        <div className="shrink-0">
          <AgentFace
            type={agentCfg.type}
            profession={(agentCfg as any).profession}
            size={40}
            mood={isStreaming && streamingPhase === 'writing' ? 'happy' : 'neutral'}
          />
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1.5">
            <h3 className="truncate text-sm font-semibold">{agent}</h3>
            {agent && <CopyButton value={agent} label="copiar nome do agente" />}
            <Badge color={TYPE_BADGE[type] ?? 'gray'} variant="light">{type}</Badge>
          </div>
          <p className="truncate text-xs text-muted-foreground">
            {humanName && <span className="text-slate-500">Você, {humanName} · </span>}
            {isStreaming ? (
              streamingPhase === 'thinking' ? (
                <span className="inline-flex items-center gap-1 text-amber-600">
                  <Brain className="h-3 w-3" /> pensando…
                </span>
              ) : (
                <span className="text-emerald-600">digitando…</span>
              )
            ) : (
              agentCfg.role || 'agente'
            )}
          </p>
        </div>
        {onToggleCollapse && (
          <ActionIcon variant="subtle" color="gray" onClick={onToggleCollapse} label="minimizar chat">
            <PanelRightClose className="h-4 w-4" />
          </ActionIcon>
        )}
        {onClose && (
          <ActionIcon variant="subtle" color="gray" onClick={onClose} label="fechar">
            <X className="h-4 w-4" />
          </ActionIcon>
        )}
      </header>

      <div
        ref={scrollRef}
        onScroll={onScroll}
        className="chat-scroll flex-1 space-y-2 overflow-y-auto bg-slate-50 px-4 py-4"
      >
        {hasOlder && (
          <div className="flex justify-center py-1">
            <button
              type="button"
              onClick={() => setVisibleCount((c) => Math.min(messages.length, c + BATCH))}
              className="rounded-full bg-white px-3 py-1 text-xs font-medium text-slate-500 shadow-sm hover:bg-slate-50"
            >
              carregar mensagens anteriores
            </button>
          </div>
        )}
        {loading && grouped.length === 0 && (
          <div className="flex h-full flex-col items-center justify-center gap-3 text-center text-xs text-muted-foreground">
            <Loader size="md" color="primary" />
            <p>Carregando conversa com {agent}…</p>
          </div>
        )}
        {!loading && grouped.length === 0 && (
          <div className="flex h-full flex-col items-center justify-center text-center text-xs text-muted-foreground">
            <Bot className="mb-2 h-8 w-8 opacity-30" />
            <p>Nenhuma mensagem ainda. Pergunte algo ao {agent}.</p>
          </div>
        )}
        {grouped.map((item, i) => {
          if (item.kind === 'day') {
            return <DayDivider key={`d${i}`} label={item.label} />;
          }
          const inThread = item.threadId && item.threadSize > 1;
          const collapsed = inThread && collapsedThreads.has(item.threadId!);
          const isThreadHead = inThread && item.threadIndex === 0;
          if (collapsed && !isThreadHead) return null;
          return (
            <div
              key={i}
              className={cn(
                inThread && item.threadIndex > 0 && 'ml-6 border-l-2 border-slate-200 pl-3',
              )}
            >
              {isThreadHead && (
                <button
                  type="button"
                  onClick={() => toggleThread(item.threadId!)}
                  className="mb-1 inline-flex items-center gap-1 rounded-full bg-slate-100 px-2 py-0.5 text-xs font-medium text-slate-600 hover:bg-slate-200"
                  title={collapsed ? 'expandir thread' : 'recolher thread'}
                >
                  {collapsed ? '▸' : '▾'} thread · {item.threadSize} msgs
                </button>
              )}
              <MessageBubble
                msg={item.msg}
                humanName={humanName}
                agentType={agentCfg.type}
                agentProfession={(agentCfg as any).profession}
              />
            </div>
          );
        })}
        {err && (
          <div className="mx-auto my-2 max-w-[90%] rounded-md border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-700">
            {err}
          </div>
        )}
      </div>

      <form
        className="border-t bg-white p-3"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        {improve && improve.phase === 'ready' && (
          <div className="mb-2 rounded-xl border border-violet-200 bg-violet-50 px-3 py-2 text-sm">
            <div className="mb-1 flex items-center gap-1.5 text-xs font-semibold uppercase tracking-wide text-violet-700">
              <Sparkles className="h-3 w-3" /> sugestão de texto
            </div>
            <div className="mb-1 text-xs text-slate-500">
              <span className="font-medium text-slate-600">original:</span> {improve.original}
            </div>
            <div className="text-slate-900">{improve.improved}</div>
            <div className="mt-2 flex gap-2">
              <Button type="button" size="compact-sm" onClick={acceptImprove}>
                usar sugestão
              </Button>
              <Button
                type="button"
                size="compact-sm"
                variant="default"
                color="gray"
                onClick={() => setImprove(null)}
              >
                descartar
              </Button>
            </div>
          </div>
        )}
        {improve && improve.phase === 'error' && (
          <div className="mb-2 flex items-center justify-between rounded-xl border border-amber-200 bg-amber-50 px-3 py-1.5 text-xs text-amber-800">
            <span>{improve.message}</span>
            <button
              type="button"
              className="rounded p-0.5 text-amber-700 hover:bg-amber-100"
              onClick={() => setImprove(null)}
              aria-label="fechar"
            >
              <X className="h-3 w-3" />
            </button>
          </div>
        )}
        <div className="relative flex items-end gap-2 rounded-2xl border bg-slate-50 px-3 py-2 focus-within:border-primary focus-within:bg-white focus-within:ring-1 focus-within:ring-primary">
          {mention && filteredMentions.length > 0 && (
            <div className="absolute bottom-full left-2 right-2 z-20 mb-2 max-h-64 overflow-auto rounded-xl border bg-white py-1 shadow-lg">
              <div className="px-3 pb-1 pt-1 text-xs font-semibold uppercase tracking-wide text-slate-400">
                mencionar
              </div>
              {filteredMentions.map((t, i) => (
                <button
                  key={`${t.kind}:${t.id}`}
                  type="button"
                  className={cn(
                    'flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm',
                    i === mention.hover ? 'bg-primary/10' : 'hover:bg-slate-50',
                  )}
                  onMouseEnter={() => setMention((cur) => (cur ? { ...cur, hover: i } : cur))}
                  onMouseDown={(e) => {
                    e.preventDefault();
                    applyMention(t);
                  }}
                >
                  <span className="flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-slate-100 text-xs">
                    {t.kind === 'agent' ? (t.emoji || <Bot className="h-3 w-3 text-slate-500" />) : <UserIcon className="h-3 w-3 text-slate-500" />}
                  </span>
                  <span className="font-medium">{t.display}</span>
                  {t.subtitle && (
                    <span className="ml-auto truncate text-xs text-slate-400">{t.subtitle}</span>
                  )}
                </button>
              ))}
            </div>
          )}
          <textarea
            ref={taRef}
            value={input}
            onChange={(e) => {
              setInput(e.target.value);
              syncMention(e.target.value, e.target.selectionStart ?? e.target.value.length);
            }}
            onInput={(e) => autoResize(e.currentTarget)}
            onKeyUp={(e) => {
              const ta = e.currentTarget;
              syncMention(ta.value, ta.selectionStart ?? ta.value.length);
            }}
            onClick={(e) => {
              const ta = e.currentTarget;
              syncMention(ta.value, ta.selectionStart ?? ta.value.length);
            }}
            onBlur={() => setMention(null)}
            onKeyDown={(e) => {
              if (mention && filteredMentions.length > 0) {
                if (e.key === 'ArrowDown') {
                  e.preventDefault();
                  setMention((cur) => cur ? { ...cur, hover: (cur.hover + 1) % filteredMentions.length } : cur);
                  return;
                }
                if (e.key === 'ArrowUp') {
                  e.preventDefault();
                  setMention((cur) => cur ? { ...cur, hover: (cur.hover - 1 + filteredMentions.length) % filteredMentions.length } : cur);
                  return;
                }
                if (e.key === 'Enter' || e.key === 'Tab') {
                  e.preventDefault();
                  applyMention(filteredMentions[mention.hover] || filteredMentions[0]);
                  return;
                }
                if (e.key === 'Escape') {
                  e.preventDefault();
                  setMention(null);
                  return;
                }
              }
              if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault();
                submit();
              }
            }}
            placeholder={humanName ? `${humanName} → ${agent}…` : `Conversar com ${agent}…`}
            rows={3}
            className="chat-ta min-w-0 flex-1 resize-none bg-transparent py-1.5 text-base leading-relaxed placeholder:text-slate-400 focus:outline-none md:text-sm"
            style={{ minHeight: 72 }}
          />
          <ActionIcon
            variant="subtle"
            color="primary"
            size="lg"
            disabled={!input.trim() || sending || improve?.phase === 'loading'}
            onClick={runImprove}
            label="melhorar ortografia e clareza (sem mudar intenção)"
          >
            {improve?.phase === 'loading' ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Sparkles className="h-4 w-4" />
            )}
          </ActionIcon>
          <ActionIcon
            variant="filled"
            color="primary"
            size="lg"
            disabled={!input.trim() || sending}
            onClick={submit}
            label="enviar"
          >
            <Send className="h-4 w-4" />
          </ActionIcon>
        </div>
        <p className="mt-1.5 px-1 text-xs text-muted-foreground">
          Enter envia · Shift+Enter quebra linha · @ menciona agentes/humanos · ✨ melhora o texto
        </p>
      </form>
    </div>
  );
}
