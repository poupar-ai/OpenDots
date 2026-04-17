import { useEffect, useMemo, useRef, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { Send, X, Bot, User as UserIcon } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { cn, fmtDayKey, fmtDayLabel, fmtTime } from '@/lib/utils';
import type { AgentCfg } from '@/lib/types';
import { useChat } from '@/hooks/useChat';

interface Props {
  agent: string | null;
  agentCfg: AgentCfg | null;
  onClose?: () => void;
  onStreamingChange?: (streaming: boolean) => void;
}

const TYPE_BADGE: Record<string, 'default' | 'secondary' | 'destructive' | 'success' | 'warning' | 'outline'> = {
  core: 'default',
  specialist: 'secondary',
  integrator: 'success',
  motor: 'warning',
  firefighter: 'destructive',
};

export function ChatPanel({ agent, agentCfg, onClose, onStreamingChange }: Props) {
  const { messages, send, sending, err, isStreaming } = useChat(agent);
  const [input, setInput] = useState('');
  const scrollRef = useRef<HTMLDivElement>(null);
  const taRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    onStreamingChange?.(isStreaming);
  }, [isStreaming, onStreamingChange]);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const nearBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 120;
    if (nearBottom) el.scrollTop = el.scrollHeight;
  }, [messages]);

  const autoResize = (ta: HTMLTextAreaElement | null) => {
    if (!ta) return;
    ta.style.height = 'auto';
    const next = Math.min(Math.max(ta.scrollHeight, 72), 220);
    ta.style.height = next + 'px';
  };

  useEffect(() => {
    autoResize(taRef.current);
  }, [input, agent]);

  const grouped = useMemo(() => {
    let lastKey = '';
    const items: Array<{ kind: 'day'; label: string } | { kind: 'msg'; msg: any }> = [];
    for (const m of messages) {
      const k = fmtDayKey(m.ts);
      if (k !== lastKey) {
        items.push({ kind: 'day', label: fmtDayLabel(k) });
        lastKey = k;
      }
      items.push({ kind: 'msg', msg: m });
    }
    return items;
  }, [messages]);

  if (!agent || !agentCfg) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 p-6 text-center text-muted-foreground">
        <Bot className="h-10 w-10 opacity-40" />
        <p className="text-sm">Selecione um agente no canvas pra conversar</p>
      </div>
    );
  }

  const type = (agentCfg.type || 'specialist') as keyof typeof TYPE_BADGE;
  const submit = () => {
    const msg = input.trim();
    if (!msg || sending) return;
    send(msg);
    setInput('');
  };

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center gap-3 border-b bg-white px-4 py-3">
        <div
          className={cn(
            'flex h-10 w-10 items-center justify-center rounded-full text-lg ring-2',
            type === 'core' && 'bg-indigo-50 ring-indigo-200',
            type === 'specialist' && 'bg-sky-50 ring-sky-200',
            type === 'integrator' && 'bg-purple-50 ring-purple-200',
            type === 'motor' && 'bg-orange-50 ring-orange-200',
            type === 'firefighter' && 'bg-red-50 ring-red-200',
          )}
        >
          {agentCfg.emoji || '🤖'}
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <h3 className="truncate text-sm font-semibold">{agent}</h3>
            <Badge variant={TYPE_BADGE[type]}>{type}</Badge>
          </div>
          <p className="truncate text-xs text-muted-foreground">
            {isStreaming ? (
              <span className="text-emerald-600">digitando…</span>
            ) : (
              agentCfg.role || 'agente'
            )}
          </p>
        </div>
        {onClose && (
          <Button size="icon" variant="ghost" onClick={onClose} aria-label="fechar">
            <X className="h-4 w-4" />
          </Button>
        )}
      </header>

      <div
        ref={scrollRef}
        className="chat-scroll flex-1 space-y-2 overflow-y-auto bg-slate-50 px-4 py-4"
      >
        {grouped.length === 0 && (
          <div className="flex h-full flex-col items-center justify-center text-center text-xs text-muted-foreground">
            <Bot className="mb-2 h-8 w-8 opacity-30" />
            <p>Nenhuma mensagem ainda. Pergunte algo ao {agent}.</p>
          </div>
        )}
        {grouped.map((item, i) =>
          item.kind === 'day' ? (
            <div key={`d${i}`} className="flex justify-center py-2">
              <span className="rounded-full bg-white px-3 py-1 text-[11px] font-medium text-slate-500 shadow-sm">
                {item.label}
              </span>
            </div>
          ) : (
            <Bubble key={i} msg={item.msg} />
          ),
        )}
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
        <div className="flex items-end gap-2 rounded-2xl border bg-slate-50 px-3 py-2 focus-within:border-primary focus-within:bg-white focus-within:ring-1 focus-within:ring-primary">
          <textarea
            ref={taRef}
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onInput={(e) => autoResize(e.currentTarget)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault();
                submit();
              }
            }}
            placeholder={`Conversar com ${agent}…`}
            rows={3}
            className="chat-ta flex-1 resize-none bg-transparent py-1.5 text-sm leading-relaxed placeholder:text-slate-400 focus:outline-none"
            style={{ minHeight: 72 }}
          />
          <Button
            type="submit"
            size="icon"
            disabled={!input.trim() || sending}
            className="h-9 w-9 shrink-0 rounded-full"
            aria-label="enviar"
          >
            <Send className="h-4 w-4" />
          </Button>
        </div>
        <p className="mt-1.5 px-1 text-[10px] text-muted-foreground">
          Enter envia · Shift+Enter quebra linha
        </p>
      </form>
    </div>
  );
}

function Bubble({
  msg,
}: {
  msg: { role: 'user' | 'agent'; content: string; ts: string; streaming?: boolean };
}) {
  const isUser = msg.role === 'user';
  return (
    <div className={cn('flex items-end gap-2', isUser ? 'justify-end' : 'justify-start')}>
      {!isUser && (
        <div className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-slate-200 text-[10px]">
          <Bot className="h-3.5 w-3.5 text-slate-600" />
        </div>
      )}
      <div
        className={cn(
          'prose-bubble relative max-w-[85%] rounded-2xl px-3.5 py-2 text-sm shadow-sm',
          isUser
            ? 'user rounded-br-sm bg-primary text-primary-foreground'
            : 'rounded-bl-sm bg-white text-slate-800',
          msg.streaming && 'streaming-cursor',
        )}
      >
        {isUser ? (
          <div className="whitespace-pre-wrap break-words">{msg.content}</div>
        ) : (
          <ReactMarkdown remarkPlugins={[remarkGfm]}>{msg.content || ' '}</ReactMarkdown>
        )}
        <div
          className={cn(
            'mt-1 text-right text-[10px]',
            isUser ? 'text-white/70' : 'text-slate-400',
          )}
        >
          {fmtTime(msg.ts)}
        </div>
      </div>
      {isUser && (
        <div className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-primary/10">
          <UserIcon className="h-3.5 w-3.5 text-primary" />
        </div>
      )}
    </div>
  );
}
