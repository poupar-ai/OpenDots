import { useState } from 'react';
import { Dice5 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import {
  createHuman,
  randomSkin,
  HUMAN_LOCATIONS,
  type HumanLocation,
  type HumanSkin,
} from '@/lib/humans';

interface Props {
  onDone: (humanId: string) => void;
}

export function HumanOnboarding({ onDone }: Props) {
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [skills, setSkills] = useState('');
  const [important, setImportant] = useState('');
  const [neverShare, setNeverShare] = useState('');
  const [location, setLocation] = useState<HumanLocation>('devs');
  const [skin, setSkin] = useState<HumanSkin>(() => randomSkin());
  const [submitting, setSubmitting] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!name.trim() || submitting) return;
    setSubmitting(true);
    setErr(null);
    try {
      const skillsList = skills.split(',').map((s) => s.trim()).filter(Boolean);
      const neverShareList = neverShare.split(',').map((s) => s.trim()).filter(Boolean);
      const h = await createHuman({
        name: name.trim(),
        description: description.trim() || undefined,
        skills: skillsList,
        important: important.trim() || undefined,
        offline_privacy: neverShareList.length ? { never_share: neverShareList } : undefined,
        location,
        skin,
      });
      onDone(h.id);
    } catch (e: any) {
      setErr(e?.message || 'falha ao criar');
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/60 backdrop-blur-sm p-4">
      <form
        onSubmit={submit}
        className="w-full max-w-xl rounded-xl border bg-white p-6 shadow-2xl"
      >
        <h2 className="text-lg font-semibold">Quem é você?</h2>
        <p className="mt-1 text-xs text-muted-foreground">
          Seu perfil ajuda os agentes (e os outros humanos) a entender de onde você
          está falando. Pode editar depois.
        </p>

        <div className="mt-5 grid grid-cols-[1fr_auto] gap-5">
          <div className="space-y-4">
            <Field label="Nome" required>
              <Input
                autoFocus
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="Ex.: Tiago"
                maxLength={80}
              />
            </Field>

            <Field label="Quem você é">
              <textarea
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                placeholder="Ex.: fundador, decide produto e arquitetura."
                rows={2}
                className="w-full rounded-md border border-input bg-background px-3 py-2 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              />
            </Field>

            <Field label="Onde você fica no escritório">
              <select
                value={location}
                onChange={(e) => setLocation(e.target.value as HumanLocation)}
                className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              >
                {HUMAN_LOCATIONS.map((l) => (
                  <option key={l.id} value={l.id}>
                    {l.label}
                  </option>
                ))}
              </select>
            </Field>
          </div>

          <div className="flex flex-col items-center gap-2">
            <span className="text-[11px] font-medium text-slate-600">Seu personagem</span>
            <div className="rounded-lg border bg-slate-50 p-3">
              <SkinPreview skin={skin} />
            </div>
            <Button
              type="button"
              size="sm"
              variant="outline"
              className="gap-1.5"
              onClick={() => setSkin(randomSkin())}
            >
              <Dice5 className="h-3.5 w-3.5" /> aleatório
            </Button>
          </div>
        </div>

        <div className="mt-4 space-y-4">
          <Field label="Skills (separe por vírgula)">
            <Input
              value={skills}
              onChange={(e) => setSkills(e.target.value)}
              placeholder="python, produto, arquitetura"
            />
          </Field>

          <Field label="O que você considera importante">
            <textarea
              value={important}
              onChange={(e) => setImportant(e.target.value)}
              placeholder="Ex.: zero over-engineering. Código curto resolve melhor."
              rows={2}
              className="w-full rounded-md border border-input bg-background px-3 py-2 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            />
          </Field>

          <Field
            label="Quando eu estiver offline, nunca compartilhar (vírgula)"
            hint="Seu shadow responde com base no histórico — nunca citando esses assuntos."
          >
            <Input
              value={neverShare}
              onChange={(e) => setNeverShare(e.target.value)}
              placeholder="salários, senhas, contratos em andamento"
            />
          </Field>
        </div>

        {err && <p className="mt-3 text-xs text-red-600">{err}</p>}

        <div className="mt-6 flex justify-end gap-2">
          <Button type="submit" disabled={submitting || !name.trim()}>
            {submitting ? 'Salvando…' : 'Entrar'}
          </Button>
        </div>
      </form>
    </div>
  );
}

function SkinPreview({ skin }: { skin: HumanSkin }) {
  const hex = (n: number) => '#' + n.toString(16).padStart(6, '0');
  return (
    <svg width="88" height="120" viewBox="0 0 88 120" className="block">
      {/* cabelo (topo) */}
      <ellipse cx="44" cy="28" rx="22" ry="16" fill={hex(skin.hair)} />
      {/* cabeça */}
      <circle cx="44" cy="36" r="18" fill={hex(skin.skinTone)} />
      {/* franja */}
      <path d="M 26 32 Q 44 18 62 32 L 62 36 Q 44 30 26 36 Z" fill={hex(skin.hair)} />
      {/* olhos */}
      <circle cx="38" cy="36" r="1.6" fill="#0f172a" />
      <circle cx="50" cy="36" r="1.6" fill="#0f172a" />
      {/* boca */}
      <path d="M 39 44 Q 44 47 49 44" stroke="#0f172a" strokeWidth="1.2" fill="none" strokeLinecap="round" />
      {/* pescoço */}
      <rect x="40" y="52" width="8" height="6" fill={hex(skin.skinTone)} />
      {/* camisa */}
      <rect x="26" y="58" width="36" height="32" rx="4" fill={hex(skin.shirt)} />
      <rect x="26" y="58" width="36" height="6" fill="#00000022" />
      {/* braços */}
      <rect x="20" y="60" width="7" height="26" rx="3" fill={hex(skin.shirt)} />
      <rect x="61" y="60" width="7" height="26" rx="3" fill={hex(skin.shirt)} />
      <circle cx="23.5" cy="88" r="3.5" fill={hex(skin.skinTone)} />
      <circle cx="64.5" cy="88" r="3.5" fill={hex(skin.skinTone)} />
      {/* calça */}
      <rect x="28" y="90" width="14" height="24" fill={hex(skin.pants)} />
      <rect x="46" y="90" width="14" height="24" fill={hex(skin.pants)} />
      {/* sapatos */}
      <rect x="26" y="114" width="17" height="4" rx="2" fill="#0f172a" />
      <rect x="45" y="114" width="17" height="4" rx="2" fill="#0f172a" />
    </svg>
  );
}

function Field({
  label,
  hint,
  required,
  children,
}: {
  label: string;
  hint?: string;
  required?: boolean;
  children: React.ReactNode;
}) {
  return (
    <label className="block text-xs">
      <span className="mb-1 block font-medium text-slate-700">
        {label}
        {required && <span className="text-red-500"> *</span>}
      </span>
      {children}
      {hint && <span className="mt-1 block text-[11px] text-muted-foreground">{hint}</span>}
    </label>
  );
}
