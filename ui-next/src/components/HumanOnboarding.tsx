import { useEffect, useState } from 'react';
import { Dice5, UserPlus, LogIn } from 'lucide-react';
import { Button, Chip, Group, Modal, Select, Text, Textarea, TextInput } from '@/ui';
import { useMediaQuery } from '@/hooks/useMediaQuery';
import {
  createHuman,
  humanLogin,
  listHumans,
  randomSkin,
  HUMAN_LOCATIONS,
  SKIN_TONES,
  type HumanGender,
  type HumanLocation,
  type HumanOutfit,
  type HumanProfile,
  type HumanSkin,
} from '@/lib/humans';

const GENDER_OPTIONS: { id: HumanGender; label: string }[] = [
  { id: 'male', label: 'Masculino' },
  { id: 'female', label: 'Feminino' },
  { id: 'other', label: 'Outro' },
];

const OUTFIT_OPTIONS: { id: HumanOutfit; label: string }[] = [
  { id: 'pants', label: 'Calça' },
  { id: 'dress', label: 'Vestido' },
];

interface Props {
  onDone: (humanId: string) => void;
}

type Mode = 'login' | 'create';

export function HumanOnboarding({ onDone }: Props) {
  const [humans, setHumans] = useState<HumanProfile[] | null>(null);
  const [mode, setMode] = useState<Mode>('login');
  const [selectedId, setSelectedId] = useState<string>('');

  const [name, setName] = useState('');
  const [password, setPassword] = useState('');
  const [email, setEmail] = useState('');
  const [description, setDescription] = useState('');
  const [skills, setSkills] = useState('');
  const [important, setImportant] = useState('');
  const [neverShare, setNeverShare] = useState('');
  const [location, setLocation] = useState<HumanLocation>('devs');
  const [gender, setGender] = useState<HumanGender>('other');
  const [skin, setSkin] = useState<HumanSkin>(() => randomSkin());
  const [submitting, setSubmitting] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const isMobile = useMediaQuery('(max-width: 768px)');

  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const all = await listHumans();
        if (!alive) return;
        setHumans(all);
        if (all.length > 0) {
          setMode('login');
          setSelectedId(all[0].id);
        } else {
          setMode('create');
        }
      } catch {
        if (alive) {
          setHumans([]);
          setMode('create');
        }
      }
    })();
    return () => { alive = false; };
  }, []);

  async function login(e: React.FormEvent) {
    e.preventDefault();
    if (!selectedId || !password.trim() || submitting) return;
    setSubmitting(true);
    setErr(null);
    try {
      await humanLogin(selectedId, password);
      onDone(selectedId);
    } catch (e: any) {
      setErr(e?.message || 'falha no login');
    } finally {
      setSubmitting(false);
    }
  }

  async function submitCreate(e: React.FormEvent) {
    e.preventDefault();
    if (!name.trim() || password.trim().length < 4 || submitting) return;
    setSubmitting(true);
    setErr(null);
    try {
      const skillsList = skills.split(',').map((s) => s.trim()).filter(Boolean);
      const neverShareList = neverShare.split(',').map((s) => s.trim()).filter(Boolean);
      const h = await createHuman({
        name: name.trim(),
        password,
        email: email.trim() || undefined,
        description: description.trim() || undefined,
        skills: skillsList,
        important: important.trim() || undefined,
        offline_privacy: neverShareList.length ? { never_share: neverShareList } : undefined,
        location,
        gender,
        skin,
      });
      onDone(h.id);
    } catch (e: any) {
      setErr(e?.message || 'falha ao criar');
    } finally {
      setSubmitting(false);
    }
  }

  if (humans === null) {
    return (
      <Modal opened onClose={() => {}} withCloseButton={false} size="sm" title="Carregando…">
        <Text size="sm" muted>Buscando perfis…</Text>
      </Modal>
    );
  }

  const hasExisting = humans.length > 0;
  const loginMode = mode === 'login' && hasExisting;

  return (
    <Modal
      opened
      onClose={() => {}}
      withCloseButton={false}
      fullScreen={isMobile}
      size="lg"
      title={
        <div className="flex items-center gap-2 text-base font-semibold">
          {loginMode ? (
            <><LogIn className="h-4 w-4" /> Entrar</>
          ) : (
            <><UserPlus className="h-4 w-4" /> Quem é você?</>
          )}
        </div>
      }
    >
      {loginMode ? (
        <form onSubmit={login} className="space-y-4">
          <Text size="xs" muted>Selecione seu perfil. Sem senha — ambiente local.</Text>
          <Field label="Perfil">
            <Select
              value={selectedId}
              onChange={(v) => setSelectedId(v || '')}
              data={humans.map((h) => ({ value: h.id, label: h.name }))}
              placeholder="escolha um perfil"
            />
          </Field>
          <Field label="Senha">
            <TextInput
              type="password"
              value={password}
              onChange={setPassword}
              placeholder="sua senha"
            />
          </Field>
          {err && <p className="text-xs text-red-600">{err}</p>}
          <Group justify="space-between" gap="xs">
            <Button
              type="button"
              variant="default"
              color="gray"
              size="sm"
              leftIcon={<UserPlus className="h-3.5 w-3.5" />}
              onClick={() => setMode('create')}
            >
              criar novo
            </Button>
            <Button
              type="submit"
              size="sm"
              disabled={!selectedId || !password.trim() || submitting}
              loading={submitting}
              leftIcon={<LogIn className="h-4 w-4" />}
            >
              Entrar
            </Button>
          </Group>
        </form>
      ) : (
        <form
          onSubmit={submitCreate}
          className="flex flex-col"
          style={isMobile ? { maxHeight: 'calc(100dvh - 7rem)' } : undefined}
        >
          <div
            className="soft-scroll space-y-4 overflow-y-auto pr-1"
            style={isMobile ? { flex: 1 } : undefined}
          >
            <Text size="xs" muted>
              Seu perfil ajuda os agentes (e os outros humanos) a entender de onde você está
              falando. Pode editar depois.
            </Text>

            <div className="grid grid-cols-1 gap-5 md:grid-cols-[1fr_auto]">
              <div className="space-y-4">
                <Field label="Nome" required>
                  <TextInput value={name} onChange={setName} placeholder="Ex.: Tiago" />
                </Field>

                <Field label="Senha" required hint="(mín. 4 caracteres)">
                  <TextInput
                    type="password"
                    value={password}
                    onChange={setPassword}
                    placeholder="defina uma senha"
                  />
                </Field>

                <Field label="Email" hint="(opcional — usado p/ login futuro)">
                  <TextInput
                    type="email"
                    value={email}
                    onChange={setEmail}
                    placeholder="voce@exemplo.com"
                  />
                </Field>

                <Field label="Quem você é">
                  <Textarea
                    value={description}
                    onChange={setDescription}
                    placeholder="Ex.: fundador, decide produto e arquitetura."
                    minRows={2}
                  />
                </Field>

                <Field label="Onde você fica no escritório">
                  <Select
                    value={location}
                    onChange={(v) => setLocation((v as HumanLocation) || 'devs')}
                    data={HUMAN_LOCATIONS.map((l) => ({ value: l.id, label: l.label }))}
                  />
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
              </div>

              <div className="flex flex-col items-center gap-2">
                <Text size="xs" muted>Seu personagem</Text>
                <div className="rounded-lg border bg-slate-50 p-3">
                  <SkinPreview skin={skin} gender={gender} />
                </div>
                <Button
                  type="button"
                  size="sm"
                  variant="default"
                  color="gray"
                  leftIcon={<Dice5 className="h-3.5 w-3.5" />}
                  onClick={() => setSkin(randomSkin())}
                >
                  aleatório
                </Button>

                <div className="mt-1 w-full">
                  <Text size="xs" muted>Tom de pele</Text>
                  <div className="mt-1 grid grid-cols-5 gap-1.5">
                    {SKIN_TONES.map((t) => {
                      const selected = skin.skinTone === t.hex;
                      const hex = '#' + t.hex.toString(16).padStart(6, '0');
                      return (
                        <button
                          key={t.id}
                          type="button"
                          title={t.label}
                          aria-label={t.label}
                          onClick={() => setSkin({ ...skin, skinTone: t.hex })}
                          style={{ backgroundColor: hex }}
                          className={`h-6 w-6 rounded-full border-2 transition ${
                            selected
                              ? 'border-indigo-600 ring-2 ring-indigo-200'
                              : 'border-slate-300 hover:border-slate-500'
                          }`}
                        />
                      );
                    })}
                  </div>
                </div>

                <div className="mt-2 w-full">
                  <Text size="xs" muted>Roupa</Text>
                  <div className="mt-1">
                  <Group gap="xs">
                    {OUTFIT_OPTIONS.map((o) => {
                      const current = skin.outfit ?? 'pants';
                      return (
                        <Chip
                          key={o.id}
                          checked={current === o.id}
                          onChange={() => setSkin({ ...skin, outfit: o.id })}
                          variant="light"
                        >
                          {o.label}
                        </Chip>
                      );
                    })}
                  </Group>
                  </div>
                </div>
              </div>
            </div>

            <Field label="Skills (separe por vírgula)">
              <TextInput value={skills} onChange={setSkills} placeholder="python, produto, arquitetura" />
            </Field>

            <Field label="O que você considera importante">
              <Textarea
                value={important}
                onChange={setImportant}
                placeholder="Ex.: zero over-engineering. Código curto resolve melhor."
                minRows={2}
              />
            </Field>

            <Field
              label="Quando eu estiver offline, nunca compartilhar (vírgula)"
              hint="Seu shadow responde com base no histórico — nunca citando esses assuntos."
            >
              <TextInput
                value={neverShare}
                onChange={setNeverShare}
                placeholder="salários, senhas, contratos em andamento"
              />
            </Field>

            {err && <p className="text-xs text-red-600">{err}</p>}
          </div>

          {/* Rodapé fixo — ações sempre visíveis. */}
          <div className="mt-3 border-t bg-white pt-3">
            <Group justify="space-between" gap="xs">
              {hasExisting ? (
                <Button
                  type="button"
                  variant="subtle"
                  color="gray"
                  size="sm"
                  leftIcon={<LogIn className="h-3.5 w-3.5" />}
                  onClick={() => setMode('login')}
                >
                  já tenho um perfil
                </Button>
              ) : (
                <span />
              )}
              <Button
                type="submit"
                size="sm"
                disabled={submitting || !name.trim() || password.trim().length < 4}
                loading={submitting}
              >
                {submitting ? 'Salvando…' : 'Entrar'}
              </Button>
            </Group>
          </div>
        </form>
      )}
    </Modal>
  );
}

function SkinPreview({ skin, gender }: { skin: HumanSkin; gender?: HumanGender }) {
  const hex = (n: number) => '#' + n.toString(16).padStart(6, '0');
  const isFemale = gender === 'female';
  return (
    <svg width="88" height="120" viewBox="0 0 88 120" className="block">
      {/* cabelo longo atrás (feminino) — cai até os ombros */}
      {isFemale && (
        <path
          d="M 20 36 Q 12 18 30 12 Q 44 6 58 12 Q 76 18 68 36 L 68 92 Q 62 98 54 92 L 54 40 L 34 40 L 34 92 Q 26 98 20 92 Z"
          fill={hex(skin.hair)}
          stroke="#0f172a"
          strokeOpacity="0.2"
          strokeWidth="1"
        />
      )}
      {/* cabelo (topo) */}
      <ellipse cx="44" cy="28" rx={isFemale ? 24 : 22} ry={isFemale ? 17 : 16} fill={hex(skin.hair)} />
      {/* cabeça */}
      <circle cx="44" cy="36" r="18" fill={hex(skin.skinTone)} />
      {/* franja */}
      {isFemale ? (
        <path d="M 26 34 Q 36 18 52 26 Q 58 30 62 34 L 62 38 Q 44 32 26 38 Z" fill={hex(skin.hair)} />
      ) : (
        <path d="M 26 32 Q 44 18 62 32 L 62 36 Q 44 30 26 36 Z" fill={hex(skin.hair)} />
      )}
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
      {skin.outfit === 'dress' ? (
        <>
          {/* vestido trapezoidal */}
          <path
            d="M 26 90 L 62 90 L 72 114 L 16 114 Z"
            fill={hex(skin.shirt)}
            stroke="#0f172a"
            strokeOpacity="0.25"
            strokeWidth="1"
          />
          {/* pernas expostas */}
          <rect x="30" y="108" width="10" height="8" fill={hex(skin.skinTone)} />
          <rect x="48" y="108" width="10" height="8" fill={hex(skin.skinTone)} />
        </>
      ) : (
        <>
          {/* calça */}
          <rect x="28" y="90" width="14" height="24" fill={hex(skin.pants)} />
          <rect x="46" y="90" width="14" height="24" fill={hex(skin.pants)} />
        </>
      )}
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
    <div className="block text-xs">
      <span className="mb-1 block font-medium text-slate-700">
        {label}
        {required && <span className="text-red-500"> *</span>}
      </span>
      {children}
      {hint && <span className="mt-1 block text-xs text-muted-foreground">{hint}</span>}
    </div>
  );
}
