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

export function useChat(agent: string | null, humanId: string | null = null) {
  const [history, setHistory] = useState<ChatMsg[]>([]);
  const [stream, setStream] = useState<Stream | null>(null);
  const [sending, setSending] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const abortRef = useRef<AbortController | null>(null);
  const historyLenRef = useRef(0);

  useEffect(() => {
    historyLenRef.current = history.length;
  }, [history]);

  useEffect(() => {
    setStream(null);
    setHistory([]);
    setErr(null);
    historyLenRef.current = 0;
    if (!agent) return;
  }, [agent]);

  // Semi-streaming: abre SSE em /api/chat/stream enquanto o chat estiver idle.
  // Servidor empurra msgs novas (handoffs, auto-turns) em até ~500ms. Fecha o
  // stream enquanto o usuário está enviando (evita duplicar mensagens com o
  // próprio POST /api/chat, que já adiciona ao history localmente).
  useEffect(() => {
    if (!agent) return;
    if (sending || stream) return;

    const es = new EventSource(
      `/api/chat/stream?agent=${encodeURIComponent(agent)}&since_idx=${historyLenRef.current}`,
    );

    es.addEventListener('msg', (ev: MessageEvent) => {
      try {
        const payload = JSON.parse(ev.data);
        const idx: number = payload.idx;
        const msg: ChatMsg = payload.msg;
        setHistory((cur) => {
          if (idx < cur.length) return cur; // já temos
          return [...cur, msg];
        });
      } catch {}
    });

    es.onerror = () => {
      // EventSource re-conecta sozinho; nada a fazer aqui.
    };

    return () => {
      es.close();
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
