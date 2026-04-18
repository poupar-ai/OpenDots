import { useCallback, useEffect, useRef, useState } from 'react';
import type { ChatMsg } from '@/lib/types';

type Stream = {
  role: 'agent';
  content: string;
  ts: string;
  streaming: true;
  phase: 'thinking' | 'writing';
};

type MsgLike = ChatMsg | Stream;

const POLL_INTERVAL_MS = 3000;

function sameHistory(a: ChatMsg[], b: ChatMsg[]): boolean {
  if (a.length !== b.length) return false;
  if (!a.length) return true;
  const last_a = a[a.length - 1];
  const last_b = b[b.length - 1];
  return last_a.ts === last_b.ts && (last_a.content || '') === (last_b.content || '');
}

export function useChat(agent: string | null, humanId: string | null = null) {
  const [history, setHistory] = useState<ChatMsg[]>([]);
  const [stream, setStream] = useState<Stream | null>(null);
  const [sending, setSending] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const abortRef = useRef<AbortController | null>(null);

  useEffect(() => {
    setStream(null);
    setHistory([]);
    setErr(null);
    if (!agent) return;
    let cancelled = false;
    fetch(`/api/chat/history?agent=${encodeURIComponent(agent)}`)
      .then((r) => r.json())
      .then((j) => {
        if (!cancelled) setHistory(j.messages || []);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [agent]);

  // Polling: quando o chat está idle (sem stream próprio rodando),
  // checa o history a cada 3s pra captar handoffs de outros agentes
  // ou turns automáticos escritos no canal deste agente.
  useEffect(() => {
    if (!agent) return;
    if (sending || stream) return; // evita race com o próprio stream
    let cancelled = false;
    const tick = async () => {
      try {
        const r = await fetch(`/api/chat/history?agent=${encodeURIComponent(agent)}`);
        if (!r.ok) return;
        const j = await r.json();
        const next: ChatMsg[] = j.messages || [];
        if (cancelled) return;
        setHistory((cur) => (sameHistory(cur, next) ? cur : next));
      } catch {
        // silencia erro de rede; próximo tick tenta de novo
      }
    };
    const id = setInterval(tick, POLL_INTERVAL_MS);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, [agent, sending, stream]);

  const send = useCallback(
    async (message: string) => {
      if (!agent || !message.trim() || sending) return;
      setSending(true);
      setErr(null);

      const userMsg: ChatMsg = {
        ts: new Date().toISOString(),
        role: 'user',
        content: message,
      };
      setHistory((h) => [...h, userMsg]);

      // Mostra "pensando" imediatamente, antes do primeiro token chegar.
      let startTs = new Date().toISOString();
      setStream({ role: 'agent', content: '', ts: startTs, streaming: true, phase: 'thinking' });

      const ac = new AbortController();
      abortRef.current = ac;

      try {
        const r = await fetch('/api/chat', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ agent, message, human_id: humanId }),
          signal: ac.signal,
        });
        if (!r.ok || !r.body) throw new Error(`HTTP ${r.status}`);

        const reader = r.body.getReader();
        const dec = new TextDecoder();
        let buf = '';
        let acc = '';

        while (true) {
          const { value, done } = await reader.read();
          if (done) break;
          buf += dec.decode(value, { stream: true });
          const parts = buf.split(/\n\n/);
          buf = parts.pop() || '';
          for (const part of parts) {
            const evMatch = part.match(/^event:\s*(\S+)/m);
            const dataMatch = part.match(/^data:\s*(.+)$/m);
            if (!evMatch || !dataMatch) continue;
            const ev = evMatch[1];
            let payload: any = {};
            try {
              payload = JSON.parse(dataMatch[1]);
            } catch {}
            if (ev === 'start') {
              startTs = payload.ts || startTs;
            } else if (ev === 'text') {
              acc += payload.text || '';
              setStream({ role: 'agent', content: acc, ts: startTs, streaming: true, phase: 'writing' });
            } else if (ev === 'error') {
              setErr(payload.message || payload.stderr || `exit ${payload.code}`);
            } else if (ev === 'done') {
              setHistory((h) => [
                ...h,
                {
                  ts: new Date().toISOString(),
                  role: 'agent',
                  content: acc,
                  duration_ms: payload.duration_ms,
                },
              ]);
              setStream(null);
            }
          }
        }
      } catch (e: any) {
        if (e.name !== 'AbortError') setErr(e.message || 'erro no stream');
        setStream(null);
      } finally {
        setSending(false);
        abortRef.current = null;
      }
    },
    [agent, sending, humanId],
  );

  const cancel = useCallback(() => {
    abortRef.current?.abort();
  }, []);

  const messages: MsgLike[] = stream ? [...history, stream] : history;
  return {
    messages,
    send,
    sending,
    err,
    cancel,
    isStreaming: !!stream,
    streamingPhase: stream?.phase ?? null,
  };
}
