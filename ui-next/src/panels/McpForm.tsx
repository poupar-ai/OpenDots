import { useEffect, useState, type ReactNode } from 'react';
import { Button, Chip, Group, Stack, Text, TextInput, Textarea } from '@/ui';
import { KeyRound, Database } from 'lucide-react';
import {
  registerMcp,
  getMcp,
  type RegisteredMcp,
  type McpTransport,
  type McpStorage,
} from '@/lib/mcps';

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

interface Props {
  /** nome do MCP a editar; se ausente, é cadastro novo. */
  editName?: string | null;
  /** valor inicial do campo nome (pré-preenche um cadastro novo). */
  prefillName?: string;
  onCancel: () => void;
  onSaved: (name: string) => void;
}

/**
 * Formulário de cadastro/edição de um servidor MCP. Reutilizado pelo painel
 * "Servidores MCP" e pelo editor de MCPs do agente. NÃO inclui Modal — o
 * caller decide como envolver (modal próprio, drawer, inline).
 */
export function McpForm({ editName, prefillName, onCancel, onSaved }: Props) {
  const [form, setForm] = useState<Form>(() =>
    editName ? { ...emptyForm } : { ...emptyForm, name: prefillName || '' });
  const [loading, setLoading] = useState(!!editName);
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    if (!editName) return;
    let alive = true;
    setLoading(true);
    getMcp(editName)
      .then((m) => {
        if (!alive) return;
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
      })
      .catch((e: any) => { if (alive) setErr(e?.message || 'falha ao carregar (cofre trancado?)'); })
      .finally(() => { if (alive) setLoading(false); });
    return () => { alive = false; };
  }, [editName]);

  const set = (patch: Partial<Form>) => setForm((f) => ({ ...f, ...patch }));
  const stdio = form.transport === 'stdio';

  async function save() {
    setErr(null);
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
      onSaved(mcp.name);
    } catch (e: any) {
      setErr(e?.message || 'falha ao salvar');
    } finally {
      setSaving(false);
    }
  }

  if (loading) return <Text size="sm" muted>carregando…</Text>;

  return (
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
        <Button size="sm" variant="default" color="gray" onClick={onCancel} disabled={saving}>
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
  );
}

function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div>
      <label className="mb-1 block text-xs font-semibold uppercase text-slate-600">
        {label} {hint && <span className="font-normal normal-case text-slate-400">{hint}</span>}
      </label>
      {children}
    </div>
  );
}
