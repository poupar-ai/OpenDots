import { useEffect, useMemo, useState } from 'react';
import { Activity, X as XIcon, Trash2 } from 'lucide-react';
import {
  Alert,
  Badge,
  Box,
  Button,
  Code,
  Group,
  Modal,
  Paper,
  Stack,
  Text,
  TextInput,
  Title,
} from '@/ui';
import { apiFetch } from '@/lib/api';

interface Props {
  onClose: () => void;
}

type Status = 'running' | 'done' | 'failed' | 'timed_out' | 'cancelled';

interface Subagent {
  id: string;
  parent_agent: string;
  prompt_preview: string;
  status: Status;
  started_at_ms: number;
  finished_at_ms: number | null;
  duration_ms: number | null;
  result: string | null;
  error: string | null;
  batch_id: string | null;
}

const STATUS_COLOR: Record<Status, 'primary' | 'success' | 'danger' | 'warning' | 'gray'> = {
  running: 'primary',
  done: 'success',
  failed: 'danger',
  timed_out: 'warning',
  cancelled: 'gray',
};

function fmtAgo(ms: number): string {
  const sec = Math.floor((Date.now() - ms) / 1000);
  if (sec < 60) return `${sec}s atrás`;
  if (sec < 3600) return `${Math.floor(sec / 60)}min atrás`;
  return `${Math.floor(sec / 3600)}h atrás`;
}

function fmtDur(ms: number | null): string {
  if (ms == null) return '—';
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}

export function SubagentPanel({ onClose }: Props) {
  const [items, setItems] = useState<Subagent[] | null>(null);
  const [parentFilter, setParentFilter] = useState('');
  const [statusFilter, setStatusFilter] = useState<Status | ''>('');
  const [expanded, setExpanded] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  // Polling 5s só quando panel aberto. Não consome tokens — só HTTP
  // local lendo state em memória do server.
  useEffect(() => {
    let alive = true;
    const fetchOnce = async () => {
      try {
        const params = new URLSearchParams();
        if (parentFilter.trim()) params.set('parent', parentFilter.trim());
        if (statusFilter) params.set('status', statusFilter);
        params.set('limit', '100');
        const r = await apiFetch(`/api/agents/subagent?${params}`);
        const j = await r.json();
        if (!alive) return;
        setItems(j.subagents || []);
        setErr(null);
      } catch (e: any) {
        if (alive) setErr(e?.message || 'falha ao carregar');
      }
    };
    fetchOnce();
    const id = setInterval(fetchOnce, 5000);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, [parentFilter, statusFilter]);

  const summary = useMemo(() => {
    if (!items) return { running: 0, done: 0, failed: 0, total: 0 };
    const r = items.filter((i) => i.status === 'running').length;
    const d = items.filter((i) => i.status === 'done').length;
    const f = items.filter((i) =>
      ['failed', 'timed_out', 'cancelled'].includes(i.status)
    ).length;
    return { running: r, done: d, failed: f, total: items.length };
  }, [items]);

  const cancel = async (id: string) => {
    if (!window.confirm(`Cancelar subagent ${id}?`)) return;
    setBusy(true);
    try {
      // DELETE requer JWT do pai. Se a UI não tiver, vira 401 — neste
      // caso o user precisa fazer via curl ou via outro fluxo.
      const r = await apiFetch(`/api/agents/subagent/${id}`, { method: 'DELETE' });
      if (!r.ok) {
        const j = await r.json().catch(() => ({}));
        throw new Error(j.error || `HTTP ${r.status}`);
      }
    } catch (e: any) {
      setErr(e?.message || 'falha ao cancelar');
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      opened
      onClose={onClose}
      size="xl"
      title={
        <Group gap="xs">
          <Activity size={18} />
          <Box>
            <Title order={5}>Subagents</Title>
            <Text size="xs" muted>
              Workstreams paralelos disparados por agentes pai. Zero context-cost — pai não vê prompt nem context destes.
            </Text>
          </Box>
        </Group>
      }
    >
      <Stack gap="md">
        <Paper withBorder padding="md">
          <Group gap="md">
            <Badge color="primary">{summary.running} rodando</Badge>
            <Badge color="success">{summary.done} done</Badge>
            <Badge color="danger">{summary.failed} fail/timeout/cancel</Badge>
            <Text size="xs" muted>
              total: {summary.total}
            </Text>
          </Group>
          <Box mt="xs">
            <Group gap="xs">
              <TextInput
                value={parentFilter}
                onChange={setParentFilter}
                placeholder="filter por pai (ex: oracle)"
                flex={1}
              />
              <Button
                variant={statusFilter === '' ? 'filled' : 'default'}
                color="gray"
                size="xs"
                onClick={() => setStatusFilter('')}
              >
                todos
              </Button>
              <Button
                variant={statusFilter === 'running' ? 'filled' : 'default'}
                color="primary"
                size="xs"
                onClick={() => setStatusFilter('running')}
              >
                rodando
              </Button>
              <Button
                variant={statusFilter === 'done' ? 'filled' : 'default'}
                color="success"
                size="xs"
                onClick={() => setStatusFilter('done')}
              >
                done
              </Button>
            </Group>
          </Box>
        </Paper>

        {err && <Alert color="danger">{err}</Alert>}

        {items === null && (
          <Text size="sm" muted>
            carregando…
          </Text>
        )}
        {items !== null && items.length === 0 && (
          <Text size="sm" muted>
            nenhum subagent {parentFilter || statusFilter ? 'com esse filtro' : 'no momento'}.
          </Text>
        )}

        <Stack gap="xs">
          {items?.map((s) => {
            const isExp = expanded === s.id;
            return (
              <Paper key={s.id} withBorder padding="sm">
                <Group justify="space-between" gap="xs">
                  <Box flex={1}>
                    <Group gap="xs">
                      <Badge color={STATUS_COLOR[s.status]} size="xs">
                        {s.status}
                      </Badge>
                      <Text size="xs" fw={500}>
                        {s.parent_agent}
                      </Text>
                      <Text size="xs" muted>
                        · {fmtAgo(s.started_at_ms)} · dur {fmtDur(s.duration_ms)}
                      </Text>
                      {s.batch_id && (
                        <Badge size="xs" color="gray">
                          batch
                        </Badge>
                      )}
                    </Group>
                    <Box mt={4}>
                      <Text size="xs" muted>
                        {s.prompt_preview || '(sem prompt)'}
                      </Text>
                    </Box>
                  </Box>
                  <Group gap="xs">
                    <Button
                      size="xs"
                      variant="default"
                      color="gray"
                      onClick={() => setExpanded(isExp ? null : s.id)}
                    >
                      {isExp ? 'fechar' : 'ver'}
                    </Button>
                    {s.status === 'running' && (
                      <Button
                        size="xs"
                        variant="default"
                        color="danger"
                        leftIcon={<Trash2 size={12} />}
                        loading={busy}
                        onClick={() => cancel(s.id)}
                      >
                        cancelar
                      </Button>
                    )}
                  </Group>
                </Group>

                {isExp && (
                  <Box mt="xs">
                    <Text size="xs" muted>
                      id: <Code>{s.id}</Code>
                    </Text>
                    {s.result && (
                      <Box mt="xs">
                        <Text size="xs" fw={500}>
                          result:
                        </Text>
                        <Paper withBorder padding="sm">
                          <Code style={{ whiteSpace: 'pre-wrap' }}>
                            {s.result.slice(0, 2000)}
                            {s.result.length > 2000 && '\n…(truncado em 2000 chars)'}
                          </Code>
                        </Paper>
                      </Box>
                    )}
                    {s.error && (
                      <Box mt="xs">
                        <Alert color="danger">
                          <Text size="xs">
                            <Code>{s.error}</Code>
                          </Text>
                        </Alert>
                      </Box>
                    )}
                  </Box>
                )}
              </Paper>
            );
          })}
        </Stack>
      </Stack>
    </Modal>
  );
}
