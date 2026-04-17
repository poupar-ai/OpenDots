import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Send, X, Users, Ghost, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { cn, fmtTime } from '@/lib/utils';
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

export function HumansPanel({ meId, preselectedId = null, onClose }: Props) {
  const [humans, setHumans] = useState<HumanProfile[]>([]);
  const [selected, setSelected] = useState<string | null>(preselectedId);
  const [messages, setMessages] = useState<HumanChatMsg[]>([]);
  const [input, setInput] = useState('');
  const [sending, setSending] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);

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

  async function submit(e: React.FormEvent) {
    e.preventDefault();
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

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-slate-900/50 backdrop-blur-sm p-4">
      <div className="flex h-[80vh] w-full max-w-4xl overflow-hidden rounded-xl border bg-white shadow-2xl">
        <aside className="flex w-64 flex-col border-r">
          <header className="flex items-center justify-between border-b px-3 py-2.5">
            <div className="flex items-center gap-1.5 text-sm font-semibold">
              <Users className="h-4 w-4" /> Humanos
            </div>
            <Button size="icon" variant="ghost" className="h-7 w-7" onClick={onClose}>
              <X className="h-4 w-4" />
            </Button>
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
                  className="flex flex-1 items-center gap-2 px-3 py-2.5 text-left text-xs"
                >
                  <span
                    className={cn(
                      'h-2 w-2 rounded-full',
                      h.online ? 'bg-emerald-500' : 'bg-slate-300',
                    )}
                  />
                  <span className="flex-1 truncate font-medium">{h.name}</span>
                  {!h.online && (
                    <Ghost className="h-3 w-3 text-slate-400" aria-label="shadow ativa" />
                  )}
                </button>
                <Button
                  size="icon"
                  variant="ghost"
                  className="mr-1.5 h-6 w-6 shrink-0 text-slate-300 opacity-0 hover:text-red-600 group-hover:opacity-100"
                  onClick={(e) => {
                    e.stopPropagation();
                    void removeHuman(h.id, h.name);
                  }}
                  title="excluir perfil"
                  aria-label={`excluir ${h.name}`}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </Button>
              </div>
            ))}
          </div>
        </aside>

        <section className="flex flex-1 flex-col">
          {!selectedHuman ? (
            <div className="flex flex-1 items-center justify-center text-sm text-muted-foreground">
              Selecione alguém para conversar.
            </div>
          ) : (
            <>
              <header className="flex items-center gap-2 border-b px-4 py-3">
                <span
                  className={cn(
                    'h-2.5 w-2.5 rounded-full',
                    selectedHuman.online ? 'bg-emerald-500' : 'bg-slate-300',
                  )}
                />
                <h3 className="text-sm font-semibold">{selectedHuman.name}</h3>
                {!selectedHuman.online && (
                  <Badge variant="secondary" className="gap-1">
                    <Ghost className="h-3 w-3" /> shadow responde
                  </Badge>
                )}
                <Button
                  size="icon"
                  variant="ghost"
                  className="ml-auto h-7 w-7 text-slate-400 hover:text-red-600"
                  onClick={() => void removeHuman(selectedHuman.id, selectedHuman.name)}
                  title="excluir perfil"
                  aria-label="excluir perfil"
                >
                  <Trash2 className="h-4 w-4" />
                </Button>
              </header>

              <div
                ref={scrollRef}
                className="chat-scroll flex-1 space-y-2 overflow-y-auto bg-slate-50 px-4 py-4"
              >
                {messages.length === 0 && (
                  <p className="py-10 text-center text-xs text-muted-foreground">
                    Nenhuma mensagem. Mande o primeiro alô.
                  </p>
                )}
                {messages.map((m, i) => {
                  const mine = m.from === meId;
                  return (
                    <div
                      key={i}
                      className={cn(
                        'flex items-end gap-2',
                        mine ? 'justify-end' : 'justify-start',
                      )}
                    >
                      <div
                        className={cn(
                          'relative max-w-[80%] rounded-2xl px-3.5 py-2 text-sm shadow-sm',
                          mine
                            ? 'rounded-br-sm bg-primary text-primary-foreground'
                            : 'rounded-bl-sm bg-white text-slate-800',
                        )}
                      >
                        {m.shadow && !mine && (
                          <div className="mb-1 flex items-center gap-1 text-[10px] font-medium text-slate-500">
                            <Ghost className="h-3 w-3" /> shadow de {selectedHuman.name}
                          </div>
                        )}
                        <div className="whitespace-pre-wrap break-words">{m.content}</div>
                        <div
                          className={cn(
                            'mt-1 text-right text-[10px]',
                            mine ? 'text-white/70' : 'text-slate-400',
                          )}
                        >
                          {fmtTime(m.ts)}
                        </div>
                      </div>
                    </div>
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
                        void submit(e as unknown as React.FormEvent);
                      }
                    }}
                    rows={2}
                    placeholder={`Mensagem pra ${selectedHuman.name}${selectedHuman.online ? '' : ' (offline — shadow responde)'}…`}
                    className="flex-1 resize-none bg-transparent py-1.5 text-sm leading-relaxed placeholder:text-slate-400 focus:outline-none"
                  />
                  <Button
                    type="submit"
                    size="icon"
                    disabled={!input.trim() || sending}
                    className="h-9 w-9 shrink-0 rounded-full"
                  >
                    <Send className="h-4 w-4" />
                  </Button>
                </div>
              </form>
            </>
          )}
        </section>
      </div>
    </div>
  );
}
