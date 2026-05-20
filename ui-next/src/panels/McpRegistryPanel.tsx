import { useEffect, useState } from 'react';
import { Plug, Plus, Trash2, Pencil, KeyRound, Database } from 'lucide-react';
import { ActionIcon, Badge, Button, Chip, Group, Modal, Stack, Text, TextInput, Textarea } from '@/ui';
import { useMediaQuery } from '@/hooks/useMediaQuery';
import { listMcps } from '@/lib/agents';
import {
  registerMcp,
  getMcp,
  deleteMcp,
  type RegisteredMcp,
  type McpTransport,
  type McpStorage,
} from '@/lib/mcps';

interface Props {
  onClose: () => void;
}

// env: "CHAVE=valor" por linha. headers: "Nome: valor" por linha.
function parseEnv(text: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const line of text.split('\n')) {
    const t = line.trim();
    if (!t) continue;
    const eq = t.indexOf('=');
    if (eq > 0) out[t.slice(0, eq).trim()] = t.slice(eq + 1).trim();
  }
  return out;
}
function parseHeaders(text: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const line of text.split('\n')) {
    const t = line.trim();
    if (!t) continue;
    const c = t.indexOf(':');
    if (c > 0) out[t.slice(0, c).trim()] = t.slice(c + 1).trim();
  }
  return out;
}
const fmtEnv = (o?: Record<string, string>) =>
  Object.entries(o || {}).map(([k, v]) => `${k}=${v}`).join('\n');
const fmtHeaders = (o?: Record<string, string>) =>
  Object.entries(o || {}).map(([k, v]) => `${k}: ${v}`).join('\n');

type Form = {
  name: string;
  description: string;
  transport: McpTransport;
  command: string;
  args: string;
  url: string;
  env: string;
  headers: string;
  storage: McpStorage;
  isEdit: boolean;
};

const emptyForm: Form = {
  name: '', description: '', transport: 'stdio', command: '', args: '',
  url: '', env: '', headers: '', storage: 'db', isEdit: false,
};

export function McpRegistryPanel({ onClose }: Props) {
  const [list, setList] = useState<RegisteredMcp[]>([]);
  const [form, setForm] = useState<Form | null>(null);
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const isMobile = useMediaQuery('(max-width: 768px)');

  const refresh = async () => {
    try {
      const m = await listMcps();
      setList(m.registered || []);
    } catch {}
  };
  useEffect(() => { void refresh(); }, []);

  const set = (patch: Partial<Form>) => setForm((f) => (f ? { ...f, ...patch } : f));

  async function openEdit(name: string) {
    setErr(null);
    try {
      const m = await getMcp(name); // resolve vault (precisa destrancado)
      setForm({
        name: m.name,
        description: m.description || '',
        transport: m.transport,
        command: m.command || '',
        args: (m.args || []).join(' '),
        url: m.url || '',
        env: fmtEnv(m.env),
        headers: fmtHeaders(m.headers),
        storage: m.storage,
        isEdit: true,
      });
    } catch (e: any) {
      setErr(e?.message || 'falha ao carregar (cofre trancado?)');
    }
  }

  async function save() {
    if (!form) return;
    setErr(null);
    const stdio = form.transport === 'stdio';
    const mcp: RegisteredMcp = {
      name: form.name.trim(),
      transport: form.transport,
      description: form.description.trim() || undefined,
      storage: form.storage,
      ...(stdio
        ? {
            command: form.command.trim(),
            args: form.args.trim() ? form.args.trim().split(/\s+/) : [],
            env: parseEnv(form.env),
          }
        : { url: form.url.trim(), headers: parseHeaders(form.headers) }),
    };
    setSaving(true);
    try {
      await registerMcp(mcp);
      setForm(null);
      await refresh();
    } catch (e: any) {
      setErr(e?.message || 'falha ao salvar');
    } finally {
      setSaving(false);
    }
  }

  async function remove(name: string) {
    if (!window.confirm(`Remover o MCP "${name}"? Sai do registro e do .mcp.json.`)) return;
    try {
      await deleteMcp(name);
      await refresh();
    } catch (e: any) {
      setErr(e?.message || 'falha ao remover');
    }
  }

  const stdio = form?.transport === 'stdio';

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
      {!form ? (
        <Stack gap="sm">
          <Group justify="space-between">
            <Text size="xs" muted>
              MCPs registrados aqui são compartilhados com todos os agentes e
              materializados no <code>.mcp.json</code> do projeto (conexão real).
            </Text>
            <Button size="compact-sm" leftIcon={<Plus className="h-3.5 w-3.5" />} onClick={() => setForm(emptyForm)}>
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
                  <ActionIcon variant="subtle" color="gray" label="editar" onClick={() => void openEdit(m.name)}>
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
      ) : (
        <Stack gap="sm">
          <Field label="Nome" hint="[a-z0-9_-]">
            <TextInput value={form.name} onChange={(v) => set({ name: v })} placeholder="ex: resend" disabled={form.isEdit} />
          </Field>
          <Field label="Descrição">
            <TextInput value={form.description} onChange={(v) => set({ description: v })} placeholder="o que esse MCP faz" />
          </Field>
          <Field label="Transporte">
            <Group gap="xs">
              {(['stdio', 'http', 'sse'] as McpTransport[]).map((t) => (
                <Chip key={t} checked={form.transport === t} onChange={() => set({ transport: t })} variant="light">{t}</Chip>
              ))}
            </Group>
          </Field>
          {stdio ? (
            <>
              <Field label="Comando">
                <TextInput value={form.command} onChange={(v) => set({ command: v })} placeholder="npx" />
              </Field>
              <Field label="Args" hint="separados por espaço">
                <TextInput value={form.args} onChange={(v) => set({ args: v })} placeholder="-y @resend/mcp-send" />
              </Field>
              <Field label="Env" hint="CHAVE=valor por linha — use ${SECRET} p/ referenciar um secret do cofre">
                <Textarea value={form.env} onChange={(v) => set({ env: v })} placeholder="RESEND_API_KEY=${RESEND_API_KEY}" minRows={2} />
                <Text size="xs" muted style={{ marginTop: 4 }}>
                  Valor literal vai plano pro .mcp.json. Pra não expor, ponha o secret no Cofre
                  e referencie como <code>${'{'}NOME{'}'}</code> — o agente resolve no spawn (ACL).
                </Text>
              </Field>
            </>
          ) : (
            <>
              <Field label="URL">
                <TextInput value={form.url} onChange={(v) => set({ url: v })} placeholder="https://servidor/mcp" />
              </Field>
              <Field label="Headers" hint="Nome: valor por linha">
                <Textarea value={form.headers} onChange={(v) => set({ headers: v })} placeholder="Authorization: Bearer ..." minRows={2} />
              </Field>
            </>
          )}
          <Field label="Onde salvar">
            <Group gap="xs">
              <Chip checked={form.storage === 'db'} onChange={() => set({ storage: 'db' })} variant="light">
                <span className="inline-flex items-center gap-1"><Database className="h-3 w-3" /> banco</span>
              </Chip>
              <Chip checked={form.storage === 'vault'} onChange={() => set({ storage: 'vault' })} variant="light">
                <span className="inline-flex items-center gap-1"><KeyRound className="h-3 w-3" /> cofre (cifrado)</span>
              </Chip>
            </Group>
          </Field>
          {err && <Text size="xs" color="danger">{err}</Text>}
          <Group justify="flex-end" gap="xs">
            <Button size="sm" variant="default" color="gray" onClick={() => setForm(null)} disabled={saving}>
              cancelar
            </Button>
            <Button
              size="sm"
              onClick={save}
              loading={saving}
              disabled={saving || !form.name.trim() || (stdio ? !form.command.trim() : !form.url.trim())}
            >
              {form.isEdit ? 'salvar' : 'registrar'}
            </Button>
          </Group>
        </Stack>
      )}
    </Modal>
  );
}

function Field({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div>
      <label className="mb-1 block text-xs font-semibold uppercase text-slate-600">
        {label} {hint && <span className="font-normal normal-case text-slate-400">{hint}</span>}
      </label>
      {children}
    </div>
  );
}
