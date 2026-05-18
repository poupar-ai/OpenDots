import { useEffect, useState } from 'react';
import {
  ActionIcon,
  Alert,
  Badge,
  Box,
  Button,
  Card,
  Code,
  colorVar,
  CopyButton,
  Divider,
  Group,
  Modal,
  Paper,
  Radio,
  RadioGroup,
  Stack,
  Text,
  TextInput,
  Title,
  Tooltip,
} from '@/ui';
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

interface ScopeSuggestions {
  parent: string | null;
  siblings: string[];
}

export function NetworkPanel({ onClose }: Props) {
  const [cfg, setCfg] = useState<NetworkConfig | null>(null);
  const [mode, setMode] = useState<BindMode>('localhost');
  const [customHost, setCustomHost] = useState('');
  const [scope, setScope] = useState<ScopeConfig | null>(null);
  const [projectRoot, setProjectRoot] = useState('');
  const [suggestions, setSuggestions] = useState<ScopeSuggestions | null>(null);
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
        const [rNet, rScope, rSugg] = await Promise.all([
          fetch('/api/network-config'),
          fetch('/api/scope'),
          fetch('/api/scope/suggestions'),
        ]);
        const j = await rNet.json();
        const s = await rScope.json();
        const sg = await rSugg.json();
        if (!alive) return;
        setCfg(j);
        setMode(j.bind);
        setCustomHost(j.customHost || '');
        setScope(s);
        setProjectRoot(s.project_root || '');
        setSuggestions(sg);
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
            <Text size="xs" muted>
              Configuração do servidor — bind, porta, escopo do agente.
            </Text>
          </Box>
        </Group>
      }
    >
      {loading ? (
        <Text size="sm" muted>
          carregando…
        </Text>
      ) : (
        <Stack gap="md">
          {scope && (
            <Paper withBorder padding="md">
              <Group gap="xs">
                <FolderOpen size={14} />
                <Text size="sm" fw={600}>
                  Pasta do projeto (project_root)
                </Text>
                <Badge size="xs" color={scope.source === 'scope.json' ? 'primary' : 'gray'}>
                  {scope.source}
                </Badge>
              </Group>
              <Box mt="xs">
                <Text size="xs" muted>
                  Pasta que o agente recebe como cwd ao ser invocado (Read/Grep/Glob
                  operam a partir daqui). A memória continua em{' '}
                  <Code>{scope.mem_dir}</Code>.
                </Text>
              </Box>
              {suggestions && (suggestions.parent || suggestions.siblings.length > 0) && (
                <Box mt="xs">
                  <Text size="xs" muted>
                    sugestões (clique pra usar):
                  </Text>
                  <Box mt={4}>
                    <Group gap="xs">
                      {suggestions.parent && (
                        <Button
                          size="compact-xs"
                          variant="default"
                          color="gray"
                          onClick={() => setProjectRoot(suggestions.parent!)}
                        >
                          {suggestions.parent}{' '}
                          <Text component="span" size="xs" muted>
                            (workspace)
                          </Text>
                        </Button>
                      )}
                      {suggestions.siblings.map((s) => (
                        <Button
                          key={s}
                          size="compact-xs"
                          variant="default"
                          color="gray"
                          onClick={() => setProjectRoot(s)}
                        >
                          {s.split('/').pop()}
                        </Button>
                      ))}
                    </Group>
                  </Box>
                </Box>
              )}
              <Box mt="xs">
                <Group gap="xs" align="flex-end">
                  <TextInput
                    flex={1}
                    value={projectRoot}
                    onChange={setProjectRoot}
                    placeholder="ex.: /home/zion/dev/paag"
                    spellCheck={false}
                  />
                  <Button
                    leftIcon={<Save size={14} />}
                    loading={savingScope}
                    disabled={
                      !projectRoot.trim() || projectRoot.trim() === scope.project_root
                    }
                    onClick={saveScope}
                  >
                    salvar
                  </Button>
                </Group>
              </Box>
              <Box mt={4}>
                <Text size="xs" muted>
                  ativo: <Code>{scope.project_root}</Code>
                </Text>
              </Box>
              {scopeErr && (
                <Box mt="xs">
                  <Alert color="danger">{scopeErr}</Alert>
                </Box>
              )}
              {scopeMsg && (
                <Box mt="xs">
                  <Alert color="success">{scopeMsg}</Alert>
                </Box>
              )}
            </Paper>
          )}

          {cfg && (
            <Paper withBorder padding="md">
              <Text size="sm" fw={600}>
                Ativo agora
              </Text>
              <Box mt={4}>
                <Text size="xs" muted>
                  bind <Code>{cfg.activeHost}</Code> · porta <Code>{cfg.port}</Code>
                </Text>
              </Box>
              <Box mt="xs">
                <Stack gap="xs">
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
                    <Text size="xs" muted>
                      nenhum IP de LAN detectado
                    </Text>
                  )}
                </Stack>
              </Box>
            </Paper>
          )}

          <Box>
            <Text size="sm" fw={600}>
              Modo de bind
            </Text>
            <Box mt="xs">
              <RadioGroup value={mode} onChange={(v) => setMode(v as BindMode)} name="bindMode">
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
              </RadioGroup>
            </Box>

            {mode === 'custom' && (
              <Box mt="xs" pl="lg">
                <TextInput
                  value={customHost}
                  onChange={setCustomHost}
                  placeholder="ex.: 192.168.0.42"
                />
                {cfg && cfg.lanIps.length > 0 && (
                  <Box mt={6}>
                    <Group gap="xs">
                      {cfg.lanIps.map((ip) => (
                        <Button
                          key={ip.address}
                          size="compact-xs"
                          variant="default"
                          color="gray"
                          onClick={() => setCustomHost(ip.address)}
                        >
                          {ip.address} ({ip.iface})
                        </Button>
                      ))}
                    </Group>
                  </Box>
                )}
              </Box>
            )}
          </Box>

          {(mode === 'lan' || mode === 'custom') && (
            <Alert
              icon={<AlertTriangle size={16} />}
              color="warning"
              title="Aviso de segurança"
            >
              Expor o servidor na rede permite que outras máquinas acessem a memória do
              projeto e o cofre de secrets. Use apenas em redes confiáveis.
            </Alert>
          )}

          {err && <Alert color="danger">{err}</Alert>}
          {savedMsg && <Alert color="success">{savedMsg}</Alert>}

          <Divider />

          <Group justify="space-between">
            <Text size="xs" muted>
              {pendingChange
                ? 'alterações pendentes — salve e reinicie o servidor'
                : 'sem alterações'}
            </Text>
            <Button
              leftIcon={<Save size={14} />}
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
    <Group justify="space-between" gap="xs" wrap="nowrap">
      <Code style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis' }}>{url}</Code>
      <Group gap="xs" wrap="nowrap">
        {label && (
          <Text size="xs" muted>
            {label}
          </Text>
        )}
        <CopyButton value={url}>
          {({ copied, copy }) => (
            <Tooltip label={copied ? 'copiado' : 'copiar'}>
              <ActionIcon onClick={copy} label="copiar URL">
                {copied ? <Check size={14} /> : <Copy size={14} />}
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
      style={{
        cursor: 'pointer',
        borderColor: active ? colorVar(warning ? 'warning' : 'primary', 5) : undefined,
        backgroundColor: active ? colorVar(warning ? 'warning' : 'primary', 0) : undefined,
      }}
    >
      <Group align="flex-start" gap="xs" wrap="nowrap">
        <Radio value={value} />
        <Box>
          <Text size="sm" fw={500}>
            {title}
          </Text>
          <Text size="xs" muted>
            {desc}
          </Text>
        </Box>
      </Group>
    </Card>
  );
}
