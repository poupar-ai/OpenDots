import { useEffect, useState } from 'react';
import { Github, Plug, Plus, Trash2, Pencil } from 'lucide-react';
import { ActionIcon, Badge, Box, Button, Group, Modal, Paper, Stack, Text } from '@/ui';
import { useMediaQuery } from '@/hooks/useMediaQuery';
import { listMcps } from '@/lib/agents';
import { deleteMcp, type RegisteredMcp } from '@/lib/mcps';
import { McpForm } from './McpForm';
import { GithubToolForm } from './GithubToolForm';

interface Props {
  onClose: () => void;
  /** Agentes do time — destino das ferramentas vindas do GitHub. */
  agentNames?: string[];
}

export function McpRegistryPanel({ onClose, agentNames = [] }: Props) {
  const [list, setList] = useState<RegisteredMcp[]>([]);
  // null = lista; { name: null } = novo; { name: 'x' } = editar.
  const [editing, setEditing] = useState<{ name: string | null } | null>(null);
  const [fromGithub, setFromGithub] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const isMobile = useMediaQuery('(max-width: 768px)');

  const refresh = async () => {
    try {
      const m = await listMcps();
      setList(m.registered || []);
    } catch {}
  };
  useEffect(() => { void refresh(); }, []);

  async function remove(name: string) {
    if (!window.confirm(`Remover o MCP "${name}"? Sai do registro e do .mcp.json.`)) return;
    try {
      await deleteMcp(name);
      await refresh();
    } catch (e: any) {
      setErr(e?.message || 'falha ao remover');
    }
  }

  return (
    <Modal
      opened
      onClose={onClose}
      fullScreen={isMobile}
      size="lg"
      title={
        <Group gap="xs">
          <Plug size={18} />
          <Text fw={600}>Servidores MCP</Text>
        </Group>
      }
    >
      {fromGithub ? (
        <GithubToolForm
          agentNames={agentNames}
          onCancel={() => setFromGithub(false)}
          onSaved={(r) => {
            setFromGithub(false);
            const who = r.attached?.length ? ` para ${r.attached.join(', ')}` : '';
            setNotice(`"${r.mcp.name}" instalado${who}.`);
            void refresh();
          }}
        />
      ) : editing ? (
        <McpForm
          editName={editing.name}
          onCancel={() => setEditing(null)}
          onSaved={() => { setEditing(null); void refresh(); }}
        />
      ) : (
        <Stack gap="sm">
          <Group justify="space-between">
            <Text size="xs" muted>
              MCPs registrados aqui são compartilhados com todos os agentes e
              materializados no <code>.mcp.json</code> do projeto (conexão real).
            </Text>
            <Group gap="xs" wrap="nowrap">
              <Button size="compact-sm" variant="light" leftIcon={<Github size={14} />} onClick={() => { setNotice(null); setFromGithub(true); }}>
                do GitHub
              </Button>
              <Button size="compact-sm" leftIcon={<Plus size={14} />} onClick={() => setEditing({ name: null })}>
                novo
              </Button>
            </Group>
          </Group>
          {err && <Text size="xs" color="danger">{err}</Text>}
          {notice && <Text size="xs" color="success">{notice}</Text>}
          {list.length === 0 ? (
            <Text size="sm" muted>Nenhum MCP registrado ainda.</Text>
          ) : (
            <Stack gap={6}>
              {list.map((m) => (
                <Paper key={m.name} withBorder padding="xs" px="sm">
                  <Group gap="xs" wrap="nowrap">
                    <Box style={{ minWidth: 0, flex: 1 }}>
                      <Group gap={6} wrap="nowrap">
                        <Text fw={500} style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{m.name}</Text>
                        <Badge color="gray" variant="light" size="sm">{m.transport}</Badge>
                        <Badge color={m.storage === 'vault' ? 'warning' : 'info'} variant="light" size="sm">
                          {m.storage === 'vault' ? 'cofre' : 'banco'}
                        </Badge>
                      </Group>
                      {m.description && (
                        <Text size="xs" muted style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                          {m.description}
                        </Text>
                      )}
                    </Box>
                    <ActionIcon variant="subtle" color="gray" label="editar" onClick={() => setEditing({ name: m.name })}>
                      <Pencil size={16} />
                    </ActionIcon>
                    <ActionIcon variant="subtle" color="danger" label="remover" onClick={() => void remove(m.name)}>
                      <Trash2 size={16} />
                    </ActionIcon>
                  </Group>
                </Paper>
              ))}
            </Stack>
          )}
        </Stack>
      )}
    </Modal>
  );
}
