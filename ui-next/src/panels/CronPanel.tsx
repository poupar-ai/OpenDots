import { useEffect, useMemo, useState } from 'react';
import { Clock, Plus, Trash2, AlertCircle, Check } from 'lucide-react';
import {
  Alert,
  Badge,
  Box,
  Button,
  Group,
  Loader,
  Modal,
  Paper,
  Select,
  Stack,
  Text,
  Textarea,
  TextInput,
  Title,
} from '@/ui';
import {
  createCronJob,
  deleteCronJob,
  listCronJobs,
  SCHEDULE_PRESETS,
  type CronJob,
} from '@/lib/cron';
import type { AgentsData } from '@/lib/types';

interface Props {
  onClose: () => void;
  /** Agents do projeto pra popular os dropdowns owner/target. */
  agents: AgentsData | null;
}

function fmtAbsolute(ms: number | null): string {
  if (!ms) return '—';
  return new Date(ms).toLocaleString('pt-BR');
}

function fmtRelative(ms: number | null): string {
  if (!ms) return '—';
  const delta = ms - Date.now();
  const abs = Math.abs(delta);
  const sec = Math.floor(abs / 1000);
  const min = Math.floor(sec / 60);
  const hr = Math.floor(min / 60);
  const day = Math.floor(hr / 24);
  const sign = delta >= 0 ? 'em' : 'há';
  if (sec < 60) return `${sign} ${sec}s`;
  if (min < 60) return `${sign} ${min}min`;
  if (hr < 48) return `${sign} ${hr}h`;
  return `${sign} ${day}d`;
}

export function CronPanel({ onClose, agents }: Props) {
  const [jobs, setJobs] = useState<CronJob[] | null>(null);
  const [loadErr, setLoadErr] = useState<string | null>(null);

  // Form state
  const [mode, setMode] = useState<'once' | 'recurring'>('once');
  const [owner, setOwner] = useState<string | null>(null);
  const [target, setTarget] = useState<string | null>(null);
  const [schedule, setSchedule] = useState<string>('@hourly');
  const [customExpr, setCustomExpr] = useState<string>('');
  const [delayValue, setDelayValue] = useState<string>('2');
  const [delayUnit, setDelayUnit] = useState<string>('m');
  const [message, setMessage] = useState<string>('');
  const [saving, setSaving] = useState(false);
  const [saveErr, setSaveErr] = useState<string | null>(null);
  const [savedFlash, setSavedFlash] = useState<string | null>(null);

  const agentOptions = useMemo(() => {
    if (!agents?.manifest?.agents) return [];
    return Object.keys(agents.manifest.agents)
      .sort()
      .map((name) => ({ value: name, label: name }));
  }, [agents]);

  // Preset dropdown (com opção "custom")
  const scheduleOptions = [
    ...SCHEDULE_PRESETS.map((p) => ({ value: p.expr, label: p.label })),
    { value: '__custom__', label: 'Outro (cron expr customizado)' },
  ];

  async function refresh() {
    try {
      const list = await listCronJobs();
      setJobs(list);
      setLoadErr(null);
    } catch (e: any) {
      setLoadErr(e?.message || 'falha ao carregar');
    }
  }

  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const list = await listCronJobs();
        if (alive) {
          setJobs(list);
          setLoadErr(null);
        }
      } catch (e: any) {
        if (alive) setLoadErr(e?.message || 'falha ao carregar');
      }
    })();
    const id = setInterval(() => {
      void refresh();
    }, 10_000);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, []);

  const resolvedSchedule =
    schedule === '__custom__' ? customExpr.trim() : schedule;

  const delaySeconds = useMemo(() => {
    const n = parseInt(delayValue, 10);
    if (!Number.isFinite(n) || n <= 0) return 0;
    const mult: Record<string, number> = { s: 1, m: 60, h: 3600, d: 86400 };
    return n * (mult[delayUnit] || 1);
  }, [delayValue, delayUnit]);

  const canSave =
    !!owner &&
    !!target &&
    (mode === 'recurring' ? !!resolvedSchedule : delaySeconds > 0) &&
    message.trim().length > 0 &&
    !saving;

  async function submit() {
    if (!canSave || !owner || !target) return;
    setSaving(true);
    setSaveErr(null);
    setSavedFlash(null);
    try {
      const created = await createCronJob(
        mode === 'recurring'
          ? { owner, target, schedule: resolvedSchedule, message: message.trim() }
          : { owner, target, run_in_seconds: delaySeconds, message: message.trim() }
      );
      const when = created.one_shot
        ? `dispara em ${fmtAbsolute(created.run_at_ms)}`
        : `próximo: ${fmtAbsolute(created.next_run_ms)}`;
      setSavedFlash(`${created.id} criado — ${when}`);
      setMessage('');
      await refresh();
    } catch (e: any) {
      setSaveErr(e?.message || 'falha ao salvar');
    } finally {
      setSaving(false);
    }
  }

  async function remove(id: string) {
    if (!window.confirm(`Remover cron ${id}?`)) return;
    try {
      await deleteCronJob(id);
      await refresh();
    } catch (e: any) {
      setLoadErr(e?.message || 'falha ao remover');
    }
  }

  return (
    <Modal
      opened
      onClose={onClose}
      size="xl"
      title={
        <Group gap="sm" align="center">
          <Clock size={20} />
          <Box>
            <Title order={4}>Agendamentos (cron)</Title>
            <Text size="sm" muted>
              Jobs recorrentes — disparam via a queue de agentes
            </Text>
          </Box>
        </Group>
      }
    >
      <Stack gap="lg">
        {/* ── Form de criação ───────────────────────────────────────────── */}
        <Paper withBorder padding="md">
          <Stack gap="sm">
            <Title order={5}>Novo agendamento</Title>

            <Group gap="sm" align="flex-start" wrap="wrap">
              <Box flex={1} style={{ minWidth: 180 }}>
                <Select
                  label="Owner (quem dispara)"
                  value={owner}
                  onChange={setOwner}
                  data={agentOptions}
                  placeholder="escolha um agente"
                  searchable
                />
              </Box>
              <Box flex={1} style={{ minWidth: 180 }}>
                <Select
                  label="Target (quem recebe)"
                  value={target}
                  onChange={setTarget}
                  data={agentOptions}
                  placeholder="escolha um agente"
                  searchable
                />
              </Box>
            </Group>

            <Box>
              <Select
                label="Modo"
                value={mode}
                onChange={(v) => setMode((v as 'once' | 'recurring') || 'once')}
                data={[
                  { value: 'once', label: 'Daqui a... (one-shot)' },
                  { value: 'recurring', label: 'Recorrente (cron)' },
                ]}
              />
            </Box>

            {mode === 'once' ? (
              <Group gap="sm" align="flex-end">
                <Box flex={1} style={{ minWidth: 100 }}>
                  <TextInput
                    value={delayValue}
                    onChange={setDelayValue}
                    placeholder="2"
                    type="number"
                  />
                </Box>
                <Box style={{ minWidth: 140 }}>
                  <Select
                    value={delayUnit}
                    onChange={(v) => setDelayUnit(v || 'm')}
                    data={[
                      { value: 's', label: 'segundos' },
                      { value: 'm', label: 'minutos' },
                      { value: 'h', label: 'horas' },
                      { value: 'd', label: 'dias' },
                    ]}
                  />
                </Box>
                {delaySeconds > 0 && (
                  <Text size="xs" muted>
                    dispara em {fmtAbsolute(Date.now() + delaySeconds * 1000)}
                  </Text>
                )}
              </Group>
            ) : (
              <Box>
                <Select
                  label="Quando (recorrente)"
                  value={schedule}
                  onChange={(v) => setSchedule(v || '@hourly')}
                  data={scheduleOptions}
                />
                {schedule === '__custom__' && (
                  <Box mt="xs">
                    <TextInput
                      value={customExpr}
                      onChange={setCustomExpr}
                      placeholder='cron 6-campo: ex. "0 30 9 * * 1-5" (9:30 dias úteis)'
                    />
                    <Text size="xs" muted>
                      Formato: <code>sec min hora dom mês dow</code> · 0=domingo · suporta <code>*</code>, <code>N</code>, <code>N-M</code>, <code>N,M</code>, <code>*/N</code>
                    </Text>
                  </Box>
                )}
              </Box>
            )}

            <Textarea
              label="Mensagem"
              value={message}
              onChange={setMessage}
              placeholder="O que o owner manda pro target quando o cron disparar"
              minRows={2}
              maxRows={6}
            />

            {saveErr && (
              <Alert color="danger" icon={<AlertCircle size={14} />}>
                {saveErr}
              </Alert>
            )}
            {savedFlash && (
              <Alert color="success" icon={<Check size={14} />}>
                {savedFlash}
              </Alert>
            )}

            <Group justify="flex-end">
              <Button
                size="sm"
                disabled={!canSave}
                loading={saving}
                leftIcon={<Plus size={16} />}
                onClick={submit}
              >
                Agendar
              </Button>
            </Group>
          </Stack>
        </Paper>

        {/* ── Lista ──────────────────────────────────────────────────────── */}
        <Box>
          <Group justify="space-between" align="center">
            <Title order={5}>Jobs ativos</Title>
            <Text size="xs" muted>
              {jobs ? `${jobs.length} job${jobs.length === 1 ? '' : 's'}` : '…'}
            </Text>
          </Group>
        </Box>

        {loadErr && (
          <Alert color="danger" icon={<AlertCircle size={14} />}>
            {loadErr}
          </Alert>
        )}

        {!jobs && !loadErr && (
          <Group gap="sm">
            <Loader size="sm" />
            <Text size="sm" muted>carregando…</Text>
          </Group>
        )}

        {jobs && jobs.length === 0 && (
          <Text size="sm" muted>Nenhum agendamento ativo. Crie o primeiro acima.</Text>
        )}

        <Stack gap="sm">
          {jobs?.map((j) => (
            <Paper key={j.id} withBorder padding="sm">
              <Group justify="space-between" align="flex-start" gap="sm" wrap="nowrap">
                <Box flex={1}>
                  <Group gap="xs" align="center" wrap="wrap">
                    <Badge color="primary" variant="light">{j.owner}</Badge>
                    <Text size="sm" muted>→</Text>
                    <Badge color="info" variant="light">{j.target}</Badge>
                    {j.one_shot ? (
                      <Badge color="warning" variant="light">one-shot</Badge>
                    ) : (
                      <Text size="xs" muted style={{ fontFamily: 'monospace' }}>
                        {j.schedule}
                      </Text>
                    )}
                  </Group>
                  <Box mt="xs">
                    <Text size="sm">{j.message}</Text>
                  </Box>
                  <Box mt={4}>
                    <Text size="xs" muted>
                      próximo: {fmtAbsolute(j.next_run_ms)} ({fmtRelative(j.next_run_ms)})
                      {j.last_run_ms ? ` · último: ${fmtRelative(j.last_run_ms)}` : ''}
                    </Text>
                  </Box>
                </Box>
                <Button
                  variant="subtle"
                  color="danger"
                  size="xs"
                  leftIcon={<Trash2 size={14} />}
                  onClick={() => remove(j.id)}
                >
                  Remover
                </Button>
              </Group>
            </Paper>
          ))}
        </Stack>
      </Stack>
    </Modal>
  );
}
