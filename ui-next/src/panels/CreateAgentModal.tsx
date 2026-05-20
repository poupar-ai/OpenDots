import { useEffect, useState, type ReactNode } from 'react';
import { Save, UserPlus } from 'lucide-react';
import { Button, Chip, Group, Modal, Text, TextInput } from '@/ui';
import { useMediaQuery } from '@/hooks/useMediaQuery';
import type { AgentType, Gender } from '@/lib/types';
import { createAgent, listMcps } from '@/lib/agents';

const AGENT_TYPES: AgentType[] = ['core', 'specialist', 'integrator', 'motor', 'firefighter'];

const GENDER_OPTIONS: { id: Gender; label: string }[] = [
  { id: 'male', label: 'Masculino' },
  { id: 'female', label: 'Feminino' },
  { id: 'other', label: 'Outro' },
];

interface Props {
  existingNames: string[];
  onClose: () => void;
  onCreated?: (name: string) => void;
}

/** Label curto reaproveitado pelos campos do form. */
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

export function CreateAgentModal({ existingNames, onClose, onCreated }: Props) {
  const [name, setName] = useState('');
  const [type, setType] = useState<AgentType>('specialist');
  const [gender, setGender] = useState<Gender>('other');
  const [emoji, setEmoji] = useState('');
  const [model, setModel] = useState('sonnet-4.6');
  const [role, setRole] = useState('');
  const [tools, setTools] = useState('Read, Grep, Glob');
  const [mcps, setMcps] = useState('');
  const [canTalk, setCanTalk] = useState('manager');
  const [, setMcpSuggestions] = useState<string[]>([]);
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const isMobile = useMediaQuery('(max-width: 768px)');

  useEffect(() => {
    let alive = true;
    listMcps()
      .then((r) => { if (alive) setMcpSuggestions(r.all || []); })
      .catch(() => {});
    return () => { alive = false; };
  }, []);

  const splitList = (v: string) => v.split(/[,\n]/).map((s) => s.trim()).filter(Boolean);

  const nameClean = name.trim().toLowerCase();
  const nameValid = /^[a-z0-9_-]+$/.test(nameClean);
  const nameTaken = existingNames.includes(nameClean);
  const canSave = !!nameClean && nameValid && !nameTaken && !saving;

  const nameError = name && !nameValid
    ? 'nome inválido'
    : nameTaken
      ? `já existe um agente "${nameClean}"`
      : null;

  const save = async () => {
    if (!canSave) return;
    setErr(null);
    setSaving(true);
    try {
      await createAgent({
        name: nameClean,
        type,
        gender,
        emoji: emoji.trim() || undefined,
        model: model.trim() || undefined,
        role: role.trim() || undefined,
        tools: splitList(tools),
        mcps: splitList(mcps),
        can_talk_to: splitList(canTalk),
      });
      onCreated?.(nameClean);
      onClose();
    } catch (e: any) {
      setErr(e?.message || 'falha ao criar agente');
    } finally {
      setSaving(false);
    }
  };

  return (
    <Modal
      opened
      onClose={onClose}
      fullScreen={isMobile}
      size="lg"
      title={
        <div>
          <div className="flex items-center gap-2 text-sm font-semibold">
            <UserPlus className="h-4 w-4" /> Novo agente
          </div>
          <Text size="xs" muted>cria manifest + mini-contexto. edite depois pelo painel.</Text>
        </div>
      }
    >
      <div className="space-y-3">
        <Field label="Nome" hint='(minúsculas, números, "-", "_")'>
          <TextInput
            value={name}
            onChange={setName}
            placeholder="ex: frontend_reviewer"
            error={nameError}
          />
        </Field>

        <Field label="Tipo">
          <Group gap="xs">
            {AGENT_TYPES.map((t) => (
              <Chip key={t} checked={type === t} onChange={() => setType(t)} variant="light">
                {t}
              </Chip>
            ))}
          </Group>
        </Field>

        <Field label="Sexo">
          <Group gap="xs">
            {GENDER_OPTIONS.map((g) => (
              <Chip key={g.id} checked={gender === g.id} onChange={() => setGender(g.id)} variant="light">
                {g.label}
              </Chip>
            ))}
          </Group>
        </Field>

        <div className="grid grid-cols-1 gap-3 sm:grid-cols-[90px_1fr]">
          <Field label="Emoji">
            <TextInput value={emoji} onChange={(v) => setEmoji(v.slice(0, 4))} placeholder="🤖" />
          </Field>
          <Field label="Modelo">
            <TextInput value={model} onChange={setModel} placeholder="sonnet-4.6" />
          </Field>
        </div>

        <Field label="Papel (role)" hint="— 1 frase">
          <TextInput value={role} onChange={setRole} placeholder="ex: revisa PRs de frontend antes de merge" />
        </Field>

        <Field label="Ferramentas" hint="(separe por vírgula)">
          <TextInput value={tools} onChange={setTools} placeholder="Read, Grep, Glob" />
        </Field>

        <Field label="MCPs" hint="(separe por vírgula)">
          <TextInput value={mcps} onChange={setMcps} placeholder="ex: resend, supabase" />
        </Field>

        <Field label="Fala com" hint="(can_talk_to, vírgula)">
          <TextInput value={canTalk} onChange={setCanTalk} placeholder="manager" />
        </Field>

        {err && (
          <div className="rounded border border-red-200 bg-red-50 px-2 py-1.5 text-xs text-red-700">
            {err}
          </div>
        )}

        <Group justify="flex-end" gap="xs">
          <Button size="sm" variant="default" color="gray" onClick={onClose} disabled={saving}>
            cancelar
          </Button>
          <Button
            size="sm"
            onClick={save}
            disabled={!canSave}
            loading={saving}
            leftIcon={<Save className="h-3.5 w-3.5" />}
          >
            {saving ? 'criando…' : 'criar'}
          </Button>
        </Group>
      </div>
    </Modal>
  );
}
