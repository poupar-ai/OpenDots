import { useState } from 'react';
import { Github, Globe, Search } from 'lucide-react';
import { Alert, Badge, Button, Checkbox, Code, Group, Paper, Stack, Text, TextInput } from '@/ui';
import { installGithubTool, type GithubToolResult } from '@/lib/mcps';

interface Props {
  /** Agentes do time — quem pode receber a ferramenta. */
  agentNames: string[];
  onCancel: () => void;
  onSaved: (result: GithubToolResult) => void;
}

const LIGHTPANDA_URL = 'https://github.com/lightpanda-io/browser';

const KIND_LABEL: Record<string, string> = {
  lightpanda: 'navegador (Lightpanda)',
  npm: 'Node (npx)',
  python: 'Python (uvx)',
  cargo: 'Rust (cargo install)',
  go: 'Go (go run)',
  'npm-published': 'Node (npm, monorepo)',
  custom: 'comando personalizado',
};

/**
 * Cola a URL de qualquer repo do GitHub → vira ferramenta (MCP) dos agentes
 * escolhidos. Dois passos: "analisar" (busca + detecta, sem instalar) e
 * "instalar" (registra e anexa). Repo sem manifesto reconhecido pede o
 * comando — `{repo}` é o caminho do checkout.
 */
export function GithubToolForm({ agentNames, onCancel, onSaved }: Props) {
  const [url, setUrl] = useState('');
  const [gitRef, setGitRef] = useState('');
  const [name, setName] = useState('');
  const [command, setCommand] = useState('');
  const [args, setArgs] = useState('');
  const [agents, setAgents] = useState<string[]>([]);
  const [preview, setPreview] = useState<GithubToolResult | null>(null);
  const [needsCommand, setNeedsCommand] = useState(false);
  const [busy, setBusy] = useState<'analyze' | 'install' | null>(null);
  const [err, setErr] = useState<string | null>(null);

  const allSelected = agentNames.length > 0 && agents.length === agentNames.length;

  function request(dryRun: boolean) {
    return {
      url: url.trim(),
      ref: gitRef.trim() || undefined,
      name: name.trim() || undefined,
      command: command.trim() || undefined,
      args: command.trim() ? args.split(/\s+/).filter(Boolean) : undefined,
      agents: dryRun ? undefined : allSelected ? ('*' as const) : agents,
      dry_run: dryRun,
    };
  }

  function resetPreview() {
    setPreview(null);
    setErr(null);
  }

  async function analyze() {
    setBusy('analyze');
    setErr(null);
    try {
      const r = await installGithubTool(request(true));
      setPreview(r);
      setNeedsCommand(false);
      if (!name.trim()) setName(r.mcp.name);
    } catch (e: any) {
      const msg: string = e?.message || 'falha ao analisar';
      // Repo sem package.json/pyproject/Cargo/go.mod: pede o comando.
      if (msg.includes("informe 'command'")) setNeedsCommand(true);
      setErr(msg);
      setPreview(null);
    } finally {
      setBusy(null);
    }
  }

  async function install() {
    setBusy('install');
    setErr(null);
    try {
      onSaved(await installGithubTool(request(false)));
    } catch (e: any) {
      setErr(e?.message || 'falha ao instalar');
    } finally {
      setBusy(null);
    }
  }

  function toggle(agent: string) {
    setAgents((cur) => (cur.includes(agent) ? cur.filter((a) => a !== agent) : [...cur, agent]));
  }

  return (
    <Stack gap="sm">
      <Group gap="xs">
        <Github size={16} />
        <Text fw={600}>Ferramenta do GitHub</Text>
      </Group>
      <Text size="xs" muted>
        Cole a URL de qualquer repositório. Ele é fixado no commit atual e vira uma ferramenta
        (MCP) só dos agentes que você marcar.
      </Text>

      <Button
        size="compact-sm"
        variant="light"
        leftIcon={<Globe size={14} />}
        onClick={() => { setUrl(LIGHTPANDA_URL); setName('lightpanda'); resetPreview(); }}
      >
        navegação web (Lightpanda)
      </Button>

      <TextInput
        value={url}
        onChange={(v) => { setUrl(v); resetPreview(); }}
        placeholder="https://github.com/dono/repo"
        type="url"
        spellCheck={false}
      />
      <Group gap="xs" grow>
        <TextInput
          value={gitRef}
          onChange={(v) => { setGitRef(v); resetPreview(); }}
          placeholder="branch, tag ou commit (opcional)"
          spellCheck={false}
        />
        <TextInput value={name} onChange={setName} placeholder="nome (opcional)" spellCheck={false} />
      </Group>

      {needsCommand && (
        <Paper withBorder padding="sm">
          <Stack gap={6}>
            <Text size="xs" muted>
              Não reconheci o tipo do projeto. Diga como iniciar o servidor MCP dele —
              <Code>{'{repo}'}</Code> vira o caminho do código baixado.
            </Text>
            <TextInput value={command} onChange={setCommand} placeholder="comando (ex.: node)" spellCheck={false} />
            <TextInput
              value={args}
              onChange={setArgs}
              placeholder="argumentos (ex.: {repo}/build/index.js)"
              spellCheck={false}
            />
          </Stack>
        </Paper>
      )}

      {err && <Alert color="danger">{err}</Alert>}

      {preview && (
        <Paper withBorder padding="sm">
          <Stack gap={6}>
            <Group gap="xs">
              <Text fw={600}>{preview.mcp.name}</Text>
              <Badge color="info" variant="light" size="sm">{KIND_LABEL[preview.kind] ?? preview.kind}</Badge>
              {preview.exists && <Badge color="warning" variant="light" size="sm">substitui o existente</Badge>}
            </Group>
            {preview.mcp.description && <Text size="xs" muted>{preview.mcp.description}</Text>}
            <Code block>{[preview.mcp.command, ...(preview.mcp.args ?? [])].join(' ')}</Code>
            {preview.warnings.map((w) => (
              <Text key={w} size="xs" color="warning">{w}</Text>
            ))}
          </Stack>
        </Paper>
      )}

      {preview && (
        <Stack gap={6}>
          <Group justify="space-between">
            <Text size="sm" fw={600}>Quais agentes podem usar?</Text>
            {agentNames.length > 0 && (
              <Checkbox
                checked={allSelected}
                onChange={(on) => setAgents(on ? [...agentNames] : [])}
                label="todos"
              />
            )}
          </Group>
          {agentNames.length === 0 ? (
            <Text size="xs" muted>Nenhum agente no time ainda — a ferramenta fica no catálogo pra anexar depois.</Text>
          ) : (
            <Group gap={6}>
              {agentNames.map((a) => (
                <Checkbox key={a} checked={agents.includes(a)} onChange={() => toggle(a)} label={a} />
              ))}
            </Group>
          )}
        </Stack>
      )}

      <Group justify="flex-end" gap="xs">
        <Button variant="subtle" color="gray" onClick={onCancel}>cancelar</Button>
        {preview ? (
          <Button loading={busy === 'install'} onClick={() => void install()}>
            instalar{agents.length > 0 ? ` para ${allSelected ? 'todos' : agents.length}` : ''}
          </Button>
        ) : (
          <Button
            leftIcon={<Search size={14} />}
            loading={busy === 'analyze'}
            disabled={!url.trim() || (needsCommand && !command.trim())}
            onClick={() => void analyze()}
          >
            analisar
          </Button>
        )}
      </Group>
    </Stack>
  );
}
