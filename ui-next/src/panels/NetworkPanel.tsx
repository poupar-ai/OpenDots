import { useEffect, useState } from 'react';
import {
  ActionIcon,
  Alert,
  Badge,
  Box,
  Button,
  Card,
  Code,
  CopyButton,
  Divider,
  Group,
  Modal,
  Paper,
  Radio,
  Stack,
  Text,
  TextInput,
  Title,
  Tooltip,
} from '@mantine/core';
import {
  AlertTriangle,
  Check,
  Copy,
  FolderOpen,
  Network as NetworkIcon,
  Save,
} from 'lucide-react';

interface Props {
  onClose: () => void;
}

type BindMode = 'localhost' | 'lan' | 'custom';

interface LanIp {
  iface: string;
  address: string;
}

interface NetworkConfig {
  bind: BindMode;
  customHost: string;
  activeHost: string;
  port: number;
  lanIps: LanIp[];
}

interface ScopeConfig {
  project_root: string;
  mem_dir: string;
  source: 'scope.json' | 'default';
}

export function NetworkPanel({ onClose }: Props) {
  const [cfg, setCfg] = useState<NetworkConfig | null>(null);
  const [mode, setMode] = useState<BindMode>('localhost');
  const [customHost, setCustomHost] = useState('');
  const [scope, setScope] = useState<ScopeConfig | null>(null);
  const [projectRoot, setProjectRoot] = useState('');
  const [savingScope, setSavingScope] = useState(false);
  const [scopeMsg, setScopeMsg] = useState<string | null>(null);
  const [scopeErr, setScopeErr] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [savedMsg, setSavedMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const [rNet, rScope] = await Promise.all([
          fetch('/api/network-config'),
          fetch('/api/scope'),
        ]);
        const j = await rNet.json();
        const s = await rScope.json();
        if (!alive) return;
        setCfg(j);
        setMode(j.bind);
        setCustomHost(j.customHost || '');
        setScope(s);
        setProjectRoot(s.project_root || '');
      } catch (e: any) {
        if (alive) setErr(e?.message || 'falha ao carregar');
      } finally {
        if (alive) setLoading(false);
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  const saveScope = async () => {
    setSavingScope(true);
    setScopeErr(null);
    setScopeMsg(null);
    try {
      const r = await fetch('/api/scope', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ project_root: projectRoot.trim() }),
      });
      const j = await r.json();
      if (!r.ok) throw new Error(j.error || `HTTP ${r.status}`);
      setScopeMsg('Escopo salvo. Reinicie o servidor pra aplicar.');
      setScope({ ...(scope as ScopeConfig), project_root: j.project_root, source: 'scope.json' });
    } catch (e: any) {
      setScopeErr(e?.message || 'falha ao salvar');
    } finally {
      setSavingScope(false);
    }
  };

  const save = async () => {
    setSaving(true);
    setErr(null);
    setSavedMsg(null);
    try {
      const body: any = { bind: mode };
      if (mode === 'custom') body.customHost = customHost.trim();
      const r = await fetch('/api/network-config', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });
      const j = await r.json();
      if (!r.ok) throw new Error(j.error || `HTTP ${r.status}`);
      setSavedMsg('Salvo. Reinicie o servidor pra aplicar.');
    } catch (e: any) {
      setErr(e?.message || 'falha ao salvar');
    } finally {
      setSaving(false);
    }
  };

  const pendingChange =
    cfg &&
    (mode !== cfg.bind ||
      (mode === 'custom' && customHost.trim() !== (cfg.customHost || '')));

  return (
    <Modal
      opened
      onClose={onClose}
      size="lg"
      title={
        <Group gap="xs">
          <NetworkIcon size={18} strokeWidth={2} />
          <Box>
            <Title order={5}>Rede &amp; escopo</Title>
            <Text size="xs" c="dimmed">
              Configuração do servidor — bind, porta, escopo do agente.
            </Text>
          </Box>
        </Group>
      }
    >
      {loading ? (
        <Text size="sm" c="dimmed">
          carregando…
        </Text>
      ) : (
        <Stack gap="md">
          {scope && (
            <Paper withBorder p="md" radius="md">
              <Group gap="xs" mb={4}>
                <FolderOpen size={14} />
                <Text fw={600} size="sm">
                  Escopo do agente
                </Text>
                <Badge size="xs" variant="light" color={scope.source === 'scope.json' ? 'indigo' : 'gray'}>
                  {scope.source}
                </Badge>
              </Group>
              <Text size="xs" c="dimmed" mb="xs">
                Pasta que o agente recebe como cwd (Read/Grep/Glob operam a partir
                daqui). A memória continua em <Code>{scope.mem_dir}</Code>.
              </Text>
              <Group gap="xs" align="flex-end">
                <TextInput
                  flex={1}
                  value={projectRoot}
                  onChange={(e) => setProjectRoot(e.currentTarget.value)}
                  placeholder="ex.: /home/zion/dev/paag"
                  spellCheck={false}
                />
                <Button
                  leftSection={<Save size={14} />}
                  loading={savingScope}
                  disabled={!projectRoot.trim() || projectRoot.trim() === scope.project_root}
                  onClick={saveScope}
                >
                  salvar
                </Button>
              </Group>
              <Text size="xs" c="dimmed" mt={4}>
                ativo: <Code>{scope.project_root}</Code>
              </Text>
              {scopeErr && (
                <Alert mt="xs" color="red" variant="light" radius="sm">
                  {scopeErr}
                </Alert>
              )}
              {scopeMsg && (
                <Alert mt="xs" color="teal" variant="light" radius="sm">
                  {scopeMsg}
                </Alert>
              )}
            </Paper>
          )}

          {cfg && (
            <Paper withBorder p="md" radius="md">
              <Text fw={600} size="sm" mb={4}>
                Ativo agora
              </Text>
              <Text size="xs" c="dimmed" mb="xs">
                bind <Code>{cfg.activeHost}</Code> · porta <Code>{cfg.port}</Code>
              </Text>
              <Stack gap={4}>
                <UrlRow url={`http://localhost:${cfg.port}`} />
                {cfg.lanIps.length > 0 ? (
                  cfg.lanIps.map((ip) => (
                    <UrlRow
                      key={ip.address}
                      url={`http://${ip.address}:${cfg.port}`}
                      label={ip.iface}
                    />
                  ))
                ) : (
                  <Text size="xs" c="dimmed">
                    nenhum IP de LAN detectado
                  </Text>
                )}
              </Stack>
            </Paper>
          )}

          <Box>
            <Text fw={600} size="sm" mb="xs">
              Modo de bind
            </Text>
            <Radio.Group
              value={mode}
              onChange={(v) => setMode(v as BindMode)}
              name="bindMode"
            >
              <Stack gap="xs">
                <BindCard
                  value="localhost"
                  active={mode === 'localhost'}
                  title="Localhost (127.0.0.1)"
                  desc="Default. Só acessível deste computador. Mais seguro."
                />
                <BindCard
                  value="lan"
                  active={mode === 'lan'}
                  title="LAN (0.0.0.0)"
                  desc="Exposto em todas as interfaces de rede."
                  warning
                />
                <BindCard
                  value="custom"
                  active={mode === 'custom'}
                  title="IP específico"
                  desc="Exposto só no IP/host informado."
                  warning
                />
              </Stack>
            </Radio.Group>

            {mode === 'custom' && (
              <Box mt="xs" pl="lg">
                <TextInput
                  value={customHost}
                  onChange={(e) => setCustomHost(e.currentTarget.value)}
                  placeholder="ex.: 192.168.0.42"
                />
                {cfg && cfg.lanIps.length > 0 && (
                  <Group gap={6} mt={6}>
                    {cfg.lanIps.map((ip) => (
                      <Button
                        key={ip.address}
                        size="compact-xs"
                        variant="default"
                        onClick={() => setCustomHost(ip.address)}
                      >
                        {ip.address}{' '}
                        <Text component="span" size="xs" c="dimmed" ml={4}>
                          ({ip.iface})
                        </Text>
                      </Button>
                    ))}
                  </Group>
                )}
              </Box>
            )}
          </Box>

          {(mode === 'lan' || mode === 'custom') && (
            <Alert
              icon={<AlertTriangle size={16} />}
              color="yellow"
              variant="light"
              title="Aviso de segurança"
            >
              Expor o servidor na rede permite que outras máquinas acessem a memória do
              projeto e o cofre de secrets. Use apenas em redes confiáveis.
            </Alert>
          )}

          {err && (
            <Alert color="red" variant="light">
              {err}
            </Alert>
          )}
          {savedMsg && (
            <Alert color="teal" variant="light">
              {savedMsg}
            </Alert>
          )}

          <Divider />

          <Group justify="space-between">
            <Text size="xs" c="dimmed">
              {pendingChange
                ? 'alterações pendentes — salve e reinicie o servidor'
                : 'sem alterações'}
            </Text>
            <Button
              leftSection={<Save size={14} />}
              loading={saving}
              disabled={!pendingChange || (mode === 'custom' && !customHost.trim())}
              onClick={save}
            >
              salvar
            </Button>
          </Group>
        </Stack>
      )}
    </Modal>
  );
}

function UrlRow({ url, label }: { url: string; label?: string }) {
  return (
    <Group justify="space-between" gap="xs" px="xs" py={4} bg="white" wrap="nowrap">
      <Code style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis' }}>{url}</Code>
      <Group gap={6} wrap="nowrap">
        {label && (
          <Text size="xs" c="dimmed">
            {label}
          </Text>
        )}
        <CopyButton value={url} timeout={1500}>
          {({ copied, copy }) => (
            <Tooltip label={copied ? 'copiado' : 'copiar'} withArrow>
              <ActionIcon variant="subtle" color="gray" onClick={copy} size="sm">
                {copied ? <Check size={14} color="var(--mantine-color-teal-6)" /> : <Copy size={14} />}
              </ActionIcon>
            </Tooltip>
          )}
        </CopyButton>
      </Group>
    </Group>
  );
}

function BindCard({
  value,
  active,
  title,
  desc,
  warning,
}: {
  value: string;
  active: boolean;
  title: string;
  desc: string;
  warning?: boolean;
}) {
  return (
    <Card
      withBorder
      padding="sm"
      radius="md"
      style={{
        cursor: 'pointer',
        borderColor: active
          ? warning
            ? 'var(--mantine-color-yellow-5)'
            : 'var(--mantine-color-indigo-5)'
          : undefined,
        backgroundColor: active
          ? warning
            ? 'var(--mantine-color-yellow-0)'
            : 'var(--mantine-color-indigo-0)'
          : undefined,
      }}
    >
      <Group align="flex-start" gap="xs" wrap="nowrap">
        <Radio value={value} />
        <Box>
          <Text fw={500} size="sm">
            {title}
          </Text>
          <Text size="xs" c="dimmed">
            {desc}
          </Text>
        </Box>
      </Group>
    </Card>
  );
}
