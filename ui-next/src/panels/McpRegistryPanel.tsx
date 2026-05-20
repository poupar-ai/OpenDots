import { useEffect, useState } from 'react';
import { Plug, Plus, Trash2, Pencil } from 'lucide-react';
import { ActionIcon, Badge, Button, Group, Modal, Stack, Text } from '@/ui';
import { useMediaQuery } from '@/hooks/useMediaQuery';
import { listMcps } from '@/lib/agents';
import { deleteMcp, type RegisteredMcp } from '@/lib/mcps';
import { McpForm } from './McpForm';

interface Props {
  onClose: () => void;
}

export function McpRegistryPanel({ onClose }: Props) {
  const [list, setList] = useState<RegisteredMcp[]>([]);
  // null = lista; { name: null } = novo; { name: 'x' } = editar.
  const [editing, setEditing] = useState<{ name: string | null } | null>(null);
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
      {editing ? (
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
            <Button size="compact-sm" leftIcon={<Plus className="h-3.5 w-3.5" />} onClick={() => setEditing({ name: null })}>
              novo
            </Button>
          </Group>
          {err && <Text size="xs" color="danger">{err}</Text>}
          {list.length === 0 ? (
            <Text size="sm" muted>Nenhum MCP registrado ainda.</Text>
          ) : (
            <Stack gap={6}>
              {list.map((m) => (
                <div key={m.name} className="flex items-center gap-2 rounded-lg border bg-white px-3 py-2">
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-1.5">
                      <span className="truncate text-sm font-medium">{m.name}</span>
                      <Badge color="gray" variant="light" size="sm">{m.transport}</Badge>
                      <Badge color={m.storage === 'vault' ? 'warning' : 'info'} variant="light" size="sm">
                        {m.storage === 'vault' ? 'cofre' : 'banco'}
                      </Badge>
                    </div>
                    {m.description && <p className="truncate text-xs text-muted-foreground">{m.description}</p>}
                  </div>
                  <ActionIcon variant="subtle" color="gray" label="editar" onClick={() => setEditing({ name: m.name })}>
                    <Pencil className="h-4 w-4" />
                  </ActionIcon>
                  <ActionIcon variant="subtle" color="danger" label="remover" onClick={() => void remove(m.name)}>
                    <Trash2 className="h-4 w-4" />
                  </ActionIcon>
                </div>
              ))}
            </Stack>
          )}
        </Stack>
      )}
    </Modal>
  );
}
