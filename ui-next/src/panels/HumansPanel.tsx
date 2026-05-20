import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Send, X, Users, Ghost, Trash2, ChevronLeft } from 'lucide-react';
import { ActionIcon, Avatar, Badge } from '@/ui';
import { CopyButton } from '@/components/CopyButton';
import { MessageBubble } from '@/components/chat';
import { useMediaQuery } from '@/hooks/useMediaQuery';
import { cn } from '@/lib/utils';
import {
  listHumans,
  humanChatHistory,
  humanHandoff,
  deleteHuman,
  type HumanProfile,
  type HumanChatMsg,
} from '@/lib/humans';

interface Props {
  meId: string;
  preselectedId?: string | null;
  onClose: () => void;
}

function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (!parts.length) return '?';
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
  return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase();
}

export function HumansPanel({ meId, preselectedId = null, onClose }: Props) {
  const [humans, setHumans] = useState<HumanProfile[]>([]);
  const [selected, setSelected] = useState<string | null>(preselectedId);
  const [messages, setMessages] = useState<HumanChatMsg[]>([]);
  const [input, setInput] = useState('');
  const [sending, setSending] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const isMobile = useMediaQuery('(max-width: 768px)');

  const refreshHumans = useCallback(async () => {
    try {
      const all = await listHumans();
      setHumans(all.filter((h) => h.id !== meId));
    } catch {}
  }, [meId]);

  const refreshChat = useCallback(async () => {
    if (!selected) { setMessages([]); return; }
    try {
      const msgs = await humanChatHistory(meId, selected);
      setMessages(msgs);
    } catch {}
  }, [meId, selected]);

  useEffect(() => { void refreshHumans(); }, [refreshHumans]);
  useEffect(() => {
    if (preselectedId) setSelected(preselectedId);
  }, [preselectedId]);
  useEffect(() => {
    const t = setInterval(refreshHumans, 15_000);
    return () => clearInterval(t);
  }, [refreshHumans]);

  useEffect(() => { void refreshChat(); }, [refreshChat]);
  useEffect(() => {
    if (!selected) return;
    const t = setInterval(refreshChat, 5_000);
    return () => clearInterval(t);
  }, [selected, refreshChat]);

  useEffect(() => {
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [messages]);

  const selectedHuman = useMemo(
    () => humans.find((h) => h.id === selected) || null,
    [humans, selected],
  );

  async function submit(e?: React.FormEvent) {
    e?.preventDefault();
    const msg = input.trim();
    if (!msg || !selected || sending) return;
    setSending(true);
    try {
      await humanHandoff(meId, selected, msg);
      setInput('');
      await refreshChat();
    } catch {}
    finally { setSending(false); }
  }

  async function removeHuman(id: string, name: string) {
    const ok = window.confirm(
      `Excluir o perfil de ${name}? O histórico de conversa não é apagado.`,
    );
    if (!ok) return;
    try {
      await deleteHuman(id);
      if (selected === id) {
        setSelected(null);
        setMessages([]);
      }
      await refreshHumans();
    } catch {}
  }

  // No mobile é pane único: lista quando nada selecionado, conversa quando há.
  const showList = !isMobile || !selected;
  const showChat = !isMobile || !!selected;

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-slate-900/50 p-0 backdrop-blur-sm sm:p-4">
      <div className="flex h-full w-full max-w-4xl overflow-hidden border bg-white shadow-2xl sm:h-[80vh] sm:rounded-xl">
        {showList && (
          <aside className={cn('flex flex-col border-r', isMobile ? 'w-full' : 'w-64')}>
            <header className="flex items-center justify-between border-b px-3 py-2.5">
              <div className="flex items-center gap-1.5 text-sm font-semibold">
                <Users className="h-4 w-4" /> Humanos
              </div>
              <ActionIcon variant="subtle" color="gray" onClick={onClose} label="fechar">
                <X className="h-4 w-4" />
              </ActionIcon>
            </header>
            <div className="soft-scroll flex-1 overflow-y-auto">
              {humans.length === 0 && (
                <p className="p-4 text-xs text-muted-foreground">
                  Nenhum outro humano cadastrado ainda.
                </p>
              )}
              {humans.map((h) => (
                <div
                  key={h.id}
                  className={cn(
                    'group flex items-center border-b hover:bg-slate-50',
                    selected === h.id && 'bg-primary/5',
                  )}
                >
                  <button
                    onClick={() => setSelected(h.id)}
                    className="flex flex-1 items-center gap-2 px-3 py-3 text-left text-sm"
                  >
                    <span className={cn('h-2 w-2 rounded-full', h.online ? 'bg-emerald-500' : 'bg-slate-300')} />
                    <span className="flex-1 truncate font-medium">{h.name}</span>
                    {!h.online && <Ghost className="h-3 w-3 text-slate-400" aria-label="shadow ativa" />}
                  </button>
                  <span className="mr-1.5 shrink-0 opacity-0 group-hover:opacity-100">
                    <ActionIcon
                      variant="subtle"
                      color="danger"
                      onClick={() => void removeHuman(h.id, h.name)}
                      label={`excluir ${h.name}`}
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </ActionIcon>
                  </span>
                </div>
              ))}
            </div>
          </aside>
        )}

        {showChat && (
          <section className="flex min-w-0 flex-1 flex-col">
            {!selectedHuman ? (
              <div className="flex flex-1 items-center justify-center text-sm text-muted-foreground">
                Selecione alguém para conversar.
              </div>
            ) : (
              <>
                <header className="flex items-center gap-2 border-b px-3 py-3">
                  {isMobile && (
                    <ActionIcon variant="subtle" color="gray" onClick={() => setSelected(null)} label="voltar">
                      <ChevronLeft className="h-5 w-5" />
                    </ActionIcon>
                  )}
                  <span className={cn('h-2.5 w-2.5 rounded-full', selectedHuman.online ? 'bg-emerald-500' : 'bg-slate-300')} />
                  <h3 className="truncate text-sm font-semibold">{selectedHuman.name}</h3>
                  <CopyButton value={selectedHuman.name} label="copiar nome" />
                  {!selectedHuman.online && (
                    <Badge color="gray" variant="light" size="sm">shadow responde</Badge>
                  )}
                  <span className="ml-auto">
                    <ActionIcon
                      variant="subtle"
                      color="danger"
                      onClick={() => void removeHuman(selectedHuman.id, selectedHuman.name)}
                      label="excluir perfil"
                    >
                      <Trash2 className="h-4 w-4" />
                    </ActionIcon>
                  </span>
                </header>

                <div ref={scrollRef} className="chat-scroll flex-1 space-y-2 overflow-y-auto bg-slate-50 px-4 py-4">
                  {messages.length === 0 && (
                    <p className="py-10 text-center text-xs text-muted-foreground">
                      Nenhuma mensagem. Mande o primeiro alô.
                    </p>
                  )}
                  {messages.map((m, i) => {
                    const mine = m.from === meId;
                    return (
                      <MessageBubble
                        key={i}
                        msg={{
                          role: mine ? 'user' : 'agent',
                          content: m.shadow && !mine ? `👻 _shadow de ${selectedHuman.name}_\n\n${m.content}` : m.content,
                          ts: m.ts,
                          from: mine ? undefined : selectedHuman.name,
                        }}
                        avatar={
                          !mine ? (
                            <Avatar name={selectedHuman.name} color="gray" size={28} title={selectedHuman.name}>
                              {initials(selectedHuman.name)}
                            </Avatar>
                          ) : undefined
                        }
                      />
                    );
                  })}
                </div>

                <form className="border-t bg-white p-3" onSubmit={submit}>
                  <div className="flex items-end gap-2 rounded-2xl border bg-slate-50 px-3 py-2 focus-within:border-primary focus-within:bg-white">
                    <textarea
                      value={input}
                      onChange={(e) => setInput(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === 'Enter' && !e.shiftKey) {
                          e.preventDefault();
                          void submit();
                        }
                      }}
                      rows={2}
                      placeholder={`Mensagem pra ${selectedHuman.name}${selectedHuman.online ? '' : ' (offline — shadow responde)'}…`}
                      className="min-w-0 flex-1 resize-none bg-transparent py-1.5 text-base leading-relaxed placeholder:text-slate-400 focus:outline-none md:text-sm"
                    />
                    <ActionIcon
                      variant="filled"
                      color="primary"
                      size="lg"
                      disabled={!input.trim() || sending}
                      onClick={() => void submit()}
                      label="enviar"
                    >
                      <Send className="h-4 w-4" />
                    </ActionIcon>
                  </div>
                </form>
              </>
            )}
          </section>
        )}
      </div>
    </div>
  );
}
