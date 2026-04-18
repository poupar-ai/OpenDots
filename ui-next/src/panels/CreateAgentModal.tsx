import { useEffect, useState } from 'react';
import { X, Save, UserPlus } from 'lucide-react';
import { Button } from '@/components/ui/button';
import type { AgentType, Gender } from '@/lib/types';
import { createAgent, listMcps } from '@/lib/agents';

const AGENT_TYPES: AgentType[] = ['core', 'specialist', 'integrator', 'motor', 'firefighter'];

const GENDER_OPTIONS: { id: Gender; label: string }[] = [
  { id: 'male', label: 'Masculino' },
  { id: 'female', label: 'Feminino' },
  { id: 'other', label: 'Outro' },
];

const MODEL_SUGGESTIONS = [
  'opus',
  'sonnet',
  'haiku',
  'opus-4.7',
  'sonnet-4.6',
  'haiku-4.5',
  'claude-opus-4-7',
  'claude-sonnet-4-6',
];

const TOOL_SUGGESTIONS = ['Read', 'Grep', 'Glob', 'Edit', 'Write', 'Bash', 'WebFetch', 'WebSearch'];

interface Props {
  existingNames: string[];
  onClose: () => void;
  onCreated?: (name: string) => void;
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
  const [mcpSuggestions, setMcpSuggestions] = useState<string[]>([]);
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    listMcps()
      .then((r) => { if (alive) setMcpSuggestions(r.all || []); })
      .catch(() => {});
    return () => { alive = false; };
  }, []);

  const splitList = (v: string) =>
    v.split(/[,\n]/).map((s) => s.trim()).filter(Boolean);

  const nameClean = name.trim().toLowerCase();
  const nameValid = /^[a-z0-9_-]+$/.test(nameClean);
  const nameTaken = existingNames.includes(nameClean);

  const canSave = nameClean && nameValid && !nameTaken && !saving;

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
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/50 backdrop-blur-sm p-4">
      <div className="flex max-h-[90vh] w-full max-w-lg flex-col overflow-hidden rounded-xl border bg-white shadow-2xl">
        <header className="flex items-center justify-between border-b px-4 py-3">
          <div>
            <div className="flex items-center gap-2 text-sm font-semibold">
              <UserPlus className="h-4 w-4" /> Novo agente
            </div>
            <p className="text-xs text-muted-foreground">
              cria manifest + arquivo de mini-contexto. edite depois pelo painel.
            </p>
          </div>
          <Button size="icon" variant="ghost" className="h-7 w-7" onClick={onClose}>
            <X className="h-4 w-4" />
          </Button>
        </header>
        <div className="soft-scroll flex-1 overflow-y-auto p-4 text-xs space-y-3">
          <div>
            <label className="mb-1 block text-xs font-semibold uppercase text-slate-600">
              Nome <span className="text-slate-400">(minúsculas, números, "-", "_")</span>
            </label>
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="ex: frontend_reviewer"
              className="w-full rounded border border-slate-200 bg-white px-2 py-1.5 font-mono text-xs focus:border-indigo-400 focus:outline-none"
            />
            {name && !nameValid && (
              <p className="mt-1 text-xs text-red-600">nome inválido</p>
            )}
            {nameTaken && (
              <p className="mt-1 text-xs text-red-600">já existe um agente "{nameClean}"</p>
            )}
          </div>

          <div>
            <label className="mb-1 block text-xs font-semibold uppercase text-slate-600">Tipo</label>
            <div className="grid grid-cols-5 gap-1">
              {AGENT_TYPES.map((t) => (
                <button
                  key={t}
                  type="button"
                  onClick={() => setType(t)}
                  className={`rounded border px-1 py-1 text-xs font-medium transition ${
                    type === t ? 'border-indigo-500 bg-indigo-50 text-indigo-700' : 'border-slate-200 hover:bg-slate-50'
                  }`}
                >
                  {t}
                </button>
              ))}
            </div>
          </div>

          <div>
            <label className="mb-1 block text-xs font-semibold uppercase text-slate-600">Sexo</label>
            <div className="grid grid-cols-3 gap-1">
              {GENDER_OPTIONS.map((g) => (
                <button
                  key={g.id}
                  type="button"
                  onClick={() => setGender(g.id)}
                  className={`rounded border px-1 py-1 text-xs font-medium transition ${
                    gender === g.id ? 'border-indigo-500 bg-indigo-50 text-indigo-700' : 'border-slate-200 hover:bg-slate-50'
                  }`}
                >
                  {g.label}
                </button>
              ))}
            </div>
          </div>

          <div className="grid grid-cols-[70px_1fr] gap-2">
            <div>
              <label className="mb-1 block text-xs font-semibold uppercase text-slate-600">Emoji</label>
              <input
                value={emoji}
                onChange={(e) => setEmoji(e.target.value)}
                placeholder="🤖"
                maxLength={4}
                className="w-full rounded border border-slate-200 bg-white px-2 py-1.5 text-center text-base focus:border-indigo-400 focus:outline-none"
              />
            </div>
            <div>
              <label className="mb-1 block text-xs font-semibold uppercase text-slate-600">Modelo</label>
              <input
                list="create-model-suggestions"
                value={model}
                onChange={(e) => setModel(e.target.value)}
                placeholder="sonnet-4.6"
                className="w-full rounded border border-slate-200 bg-white px-2 py-1.5 font-mono text-xs focus:border-indigo-400 focus:outline-none"
              />
              <datalist id="create-model-suggestions">
                {MODEL_SUGGESTIONS.map((m) => <option key={m} value={m} />)}
              </datalist>
            </div>
          </div>

          <div>
            <label className="mb-1 block text-xs font-semibold uppercase text-slate-600">
              Papel (role) <span className="text-slate-400">— 1 frase</span>
            </label>
            <input
              value={role}
              onChange={(e) => setRole(e.target.value)}
              placeholder="ex: revisa PRs de frontend antes de merge"
              className="w-full rounded border border-slate-200 bg-white px-2 py-1.5 text-xs focus:border-indigo-400 focus:outline-none"
            />
          </div>

          <div>
            <label className="mb-1 block text-xs font-semibold uppercase text-slate-600">
              Ferramentas <span className="text-slate-400">(separe por vírgula)</span>
            </label>
            <input
              list="tool-suggestions"
              value={tools}
              onChange={(e) => setTools(e.target.value)}
              placeholder="Read, Grep, Glob"
              className="w-full rounded border border-slate-200 bg-white px-2 py-1.5 font-mono text-xs focus:border-indigo-400 focus:outline-none"
            />
            <datalist id="tool-suggestions">
              {TOOL_SUGGESTIONS.map((t) => <option key={t} value={t} />)}
            </datalist>
          </div>

          <div>
            <label className="mb-1 block text-xs font-semibold uppercase text-slate-600">
              MCPs <span className="text-slate-400">(separe por vírgula)</span>
            </label>
            <input
              list="create-mcp-suggestions"
              value={mcps}
              onChange={(e) => setMcps(e.target.value)}
              placeholder="ex: resend, supabase"
              className="w-full rounded border border-slate-200 bg-white px-2 py-1.5 font-mono text-xs focus:border-indigo-400 focus:outline-none"
            />
            <datalist id="create-mcp-suggestions">
              {mcpSuggestions.map((m) => <option key={m} value={m} />)}
            </datalist>
          </div>

          <div>
            <label className="mb-1 block text-xs font-semibold uppercase text-slate-600">
              Fala com <span className="text-slate-400">(can_talk_to, vírgula)</span>
            </label>
            <input
              value={canTalk}
              onChange={(e) => setCanTalk(e.target.value)}
              placeholder="manager"
              className="w-full rounded border border-slate-200 bg-white px-2 py-1.5 font-mono text-xs focus:border-indigo-400 focus:outline-none"
            />
          </div>

          {err && (
            <div className="rounded border border-red-200 bg-red-50 px-2 py-1.5 text-xs text-red-700">
              {err}
            </div>
          )}
        </div>
        <footer className="flex justify-end gap-2 border-t px-4 py-3">
          <Button size="sm" variant="outline" onClick={onClose} disabled={saving}>
            cancelar
          </Button>
          <Button size="sm" onClick={save} disabled={!canSave} className="gap-1.5">
            <Save className="h-3.5 w-3.5" />
            {saving ? 'criando…' : 'criar'}
          </Button>
        </footer>
      </div>
    </div>
  );
}
