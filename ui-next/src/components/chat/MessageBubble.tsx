import { memo, type ReactNode } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { Bot, Brain } from 'lucide-react';
import { Avatar, Paper, colorVar } from '@/ui';
import { AgentFace } from '@/components/AgentFace';
import { cn, fmtTime } from '@/lib/utils';
import { ThinkingDots } from './ThinkingDots';

export interface ChatMessageData {
  role: 'user' | 'agent';
  content: string;
  ts: string;
  streaming?: boolean;
  phase?: 'thinking' | 'writing';
  from?: string;
}

export interface MessageBubbleProps {
  msg: ChatMessageData;
  humanName?: string | null;
  agentType?: string;
  agentProfession?: string;
  /** Avatar custom do lado esquerdo (ex.: outro humano). Sobrepõe o AgentFace. */
  avatar?: ReactNode;
}

function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (!parts.length) return '?';
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
  return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase();
}

/**
 * Balão de mensagem reaproveitável (Mantine Paper + Avatar). Cobre os 3
 * estados: usuário, agente (markdown) e "pensando". Sem acoplamento ao
 * ChatPanel — recebe só os dados da mensagem e a identidade do agente, então
 * serve qualquer lista de chat (painel principal, prévia, histórico).
 */
export const MessageBubble = memo(function MessageBubble({
  msg,
  humanName,
  agentType,
  agentProfession,
  avatar,
}: MessageBubbleProps) {
  const isUser = msg.role === 'user';
  const thinking = msg.streaming && msg.phase === 'thinking';
  const writing = msg.streaming && msg.phase === 'writing';
  const userLabel = msg.from || humanName || 'Você';

  return (
    <div className={cn('flex items-end gap-2', isUser ? 'justify-end' : 'justify-start')}>
      {!isUser &&
        (thinking ? (
          <div className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-amber-100">
            <Brain className="h-3.5 w-3.5 text-amber-600" />
          </div>
        ) : avatar ? (
          avatar
        ) : agentType ? (
          <AgentFace
            type={agentType}
            profession={agentProfession}
            size={28}
            mood={writing ? 'happy' : 'neutral'}
          />
        ) : (
          <div className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-slate-200">
            <Bot className="h-3.5 w-3.5 text-slate-600" />
          </div>
        ))}

      <Paper
        px={14}
        py={8}
        radius="lg"
        shadow="xs"
        className={cn('prose-bubble', isUser && 'user', writing && 'streaming-cursor')}
        style={{
          maxWidth: '85%',
          ...(isUser
            ? { background: colorVar('primary', 6), color: 'white', borderBottomRightRadius: 4 }
            : { borderBottomLeftRadius: 4 }),
        }}
      >
        {thinking ? (
          <ThinkingDots />
        ) : isUser ? (
          <>
            <div className="mb-0.5 text-xs font-semibold" style={{ color: 'rgba(255,255,255,0.9)' }}>
              {userLabel}
            </div>
            <div className="whitespace-pre-wrap break-words">{msg.content}</div>
          </>
        ) : (
          <ReactMarkdown remarkPlugins={[remarkGfm]}>{msg.content || ' '}</ReactMarkdown>
        )}

        {!thinking && (
          <div
            className="mt-1 text-right text-xs"
            style={{ color: isUser ? 'rgba(255,255,255,0.7)' : undefined }}
          >
            <span className={isUser ? undefined : 'text-slate-400'}>{fmtTime(msg.ts)}</span>
          </div>
        )}
      </Paper>

      {isUser && <Avatar name={userLabel} color="primary" size={28} radius="xl" title={userLabel}>{initials(userLabel)}</Avatar>}
    </div>
  );
});
