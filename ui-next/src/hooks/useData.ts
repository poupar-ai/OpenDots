import { useEffect, useState, useCallback } from 'react';
import type { ServerData } from '@/lib/types';

export function useData(pollMs = 10000, paused = false) {
  const [data, setData] = useState<ServerData | null>(null);
  const [err, setErr] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const r = await fetch('/api/data');
      if (!r.ok) throw new Error(`HTTP ${r.status}`);
      const j = (await r.json()) as ServerData;
      setData(j);
      setErr(null);
    } catch (e: any) {
      setErr(e.message || 'erro ao carregar');
    }
  }, []);

  useEffect(() => {
    load();
    if (paused) return;
    const t = setInterval(load, pollMs);
    return () => clearInterval(t);
  }, [load, pollMs, paused]);

  return { data, err, refresh: load };
}
