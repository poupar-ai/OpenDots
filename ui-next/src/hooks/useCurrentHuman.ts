import { useCallback, useEffect, useState } from 'react';
import { getHuman, heartbeatHuman, type HumanProfile } from '@/lib/humans';

const STORAGE_KEY = 'open-poupar:current-human-id';
const HEARTBEAT_MS = 45_000;

export function useCurrentHuman() {
  const [humanId, setHumanId] = useState<string | null>(() => {
    try {
      return localStorage.getItem(STORAGE_KEY);
    } catch {
      return null;
    }
  });
  const [human, setHuman] = useState<HumanProfile | null>(null);
  const [loading, setLoading] = useState(false);

  const refresh = useCallback(async () => {
    if (!humanId) {
      setHuman(null);
      return;
    }
    setLoading(true);
    try {
      const h = await getHuman(humanId);
      setHuman(h);
    } catch {
      // id inválido — limpa
      try { localStorage.removeItem(STORAGE_KEY); } catch {}
      setHumanId(null);
      setHuman(null);
    } finally {
      setLoading(false);
    }
  }, [humanId]);

  useEffect(() => { void refresh(); }, [refresh]);

  useEffect(() => {
    if (!humanId) return;
    void heartbeatHuman(humanId).catch(() => {});
    const t = setInterval(() => {
      void heartbeatHuman(humanId).catch(() => {});
    }, HEARTBEAT_MS);
    return () => clearInterval(t);
  }, [humanId]);

  const setCurrent = useCallback((id: string | null) => {
    try {
      if (id) localStorage.setItem(STORAGE_KEY, id);
      else localStorage.removeItem(STORAGE_KEY);
    } catch {}
    setHumanId(id);
  }, []);

  return { humanId, human, loading, setCurrent, refresh };
}
