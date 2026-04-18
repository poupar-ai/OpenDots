import { useEffect, useState } from 'react';
import { Bot, MessageSquare, Users, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { fmtTime } from '@/lib/utils';
import { humanChatsSummary, type HumanChatsSummary } from '@/lib/humans';

interface Props {
  meId: string;
  meName: string;
  onClose: () => void;
  onOpenAgent: (name: string) => void;
  onOpenHuman: (id: string) => void;
}

export function MyChatsPanel({ meId, meName, onClose, onOpenAgent, onOpenHuman }: Props) {
  const [summary, setSummary] = useState<HumanChatsSummary | null>(null);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    const load = async () => {
      try {
        const s = await humanChatsSummary(meId);
        if (alive) setSummary(s);
      } catch (e: any) {
        if (alive) setErr(e?.message || 'falha');
      }
    };
    void load();
  }, [meId]);

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-slate-900/50 backdrop-blur-sm p-4">
      <div className="flex h-[80vh] w-full max-w-2xl flex-col overflow-hidden rounded-xl border bg-white shadow-2xl">
        <header className="flex items-center justify-between border-b px-4 py-3">
          <div>
            <div className="flex items-center gap-2 text-sm font-semibold">
              <MessageSquare className="h-4 w-4" /> Minhas conversas
            </div>
            <p className="text-xs text-muted-foreground">
              histórico de {meName}
            </p>
          </div>
          <Button size="icon" variant="ghost" className="h-7 w-7" onClick={onClose}>
            <X className="h-4 w-4" />
          </Button>
        </header>

        <div className="soft-scroll flex-1 overflow-y-auto">
          {err && (
            <div className="m-4 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-700">
              {err}
            </div>
          )}
          {!summary && !err && (
            <p className="p-6 text-center text-xs text-muted-foreground">carregando…</p>
          )}

          {summary && (
            <>
              <Section
                icon={<Bot className="h-3.5 w-3.5" />}
                title="Com agentes"
                empty="Nenhuma conversa com agentes ainda."
                count={summary.agent_chats.length}
              >
                {summary.agent_chats.map((c) => (
                  <ConversationRow
                    key={`a:${c.agent}`}
                    title={c.agent}
                    subtitle={c.last_preview}
                    ts={c.last_ts}
                    count={c.count}
                    tag="agente"
                    tagColor="bg-sky-100 text-sky-700"
                    onClick={() => {
                      onOpenAgent(c.agent);
                      onClose();
                    }}
                  />
                ))}
              </Section>

              <Section
                icon={<Users className="h-3.5 w-3.5" />}
                title="Com outros humanos"
                empty="Nenhuma conversa com outros humanos ainda."
                count={summary.human_chats.length}
              >
                {summary.human_chats.map((c) => (
                  <ConversationRow
                    key={`h:${c.peer_id}`}
                    title={c.peer_name}
                    subtitle={c.last_preview}
                    ts={c.last_ts}
                    count={c.count}
                    tag="humano"
                    tagColor="bg-emerald-100 text-emerald-700"
                    onClick={() => {
                      onOpenHuman(c.peer_id);
                      onClose();
                    }}
                  />
                ))}
              </Section>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

function Section({
  icon,
  title,
  empty,
  count,
  children,
}: {
  icon: React.ReactNode;
  title: string;
  empty: string;
  count: number;
  children: React.ReactNode;
}) {
  return (
    <div className="border-b last:border-b-0">
      <div className="sticky top-0 z-10 flex items-center gap-1.5 border-b bg-slate-50/90 px-4 py-2 text-xs font-semibold uppercase tracking-wide text-slate-500 backdrop-blur">
        {icon}
        {title}
        <span className="text-slate-400">· {count}</span>
      </div>
      {count === 0 ? (
        <p className="px-4 py-6 text-center text-xs text-muted-foreground">{empty}</p>
      ) : (
        <div>{children}</div>
      )}
    </div>
  );
}

function ConversationRow({
  title,
  subtitle,
  ts,
  count,
  tag,
  tagColor,
  onClick,
}: {
  title: string;
  subtitle: string;
  ts: string | null;
  count: number;
  tag: string;
  tagColor: string;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className="flex w-full items-start gap-3 border-b px-4 py-3 text-left last:border-b-0 hover:bg-slate-50"
    >
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          <span className="truncate text-sm font-semibold">{title}</span>
          <span className={`rounded-full px-1.5 py-0.5 text-xs font-medium ${tagColor}`}>{tag}</span>
          <span className="ml-auto text-xs text-slate-400">
            {ts ? fmtTime(ts) : ''}
          </span>
        </div>
        <p className="mt-0.5 line-clamp-2 text-xs text-slate-500">
          {subtitle || <span className="italic text-slate-400">sem preview</span>}
        </p>
        <p className="mt-1 text-xs text-slate-400">{count} mensagens</p>
      </div>
    </button>
  );
}
