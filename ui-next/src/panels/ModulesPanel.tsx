import { useMemo, useState } from 'react';
import {
  FileText, Database, AlertTriangle, ScrollText, X, Save, Pencil, Plus,
  FolderGit2, BookText, Boxes, Code2, ChevronDown, CircleCheck, CircleDashed,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import type { ServerData, AgentType } from '@/lib/types';
import { saveDoc, type DocName } from '@/lib/docs';
import { CreateAgentModal } from '@/panels/CreateAgentModal';

interface Props {
  data: ServerData | null;
}

const DOC_HINTS: Record<DocName, string> = {
  'SOUL.md': 'propósito, valores, voz, anti-padrões',
  'PROJECT.md': 'visão técnica, stack, estrutura, como rodar',
  'DECISIONS.md': 'decisões registradas com motivo',
  'GOTCHAS.md': 'armadilhas conhecidas pra quem toca o código',
  'GLOSSARY.md': 'glossário oficial dos termos do domínio',
  'CODE_INDEX.md': 'índice gerado do código',
};

const TYPE_META: Record<AgentType, { label: string; color: string; bg: string }> = {
  core:        { label: 'Core',        color: '#6366f1', bg: 'bg-indigo-50' },
  specialist:  { label: 'Specialists', color: '#0ea5e9', bg: 'bg-sky-50' },
  integrator:  { label: 'Integrators', color: '#a855f7', bg: 'bg-purple-50' },
  motor:       { label: 'Motors',      color: '#f97316', bg: 'bg-orange-50' },
  firefighter: { label: 'Firefighters', color: '#ef4444', bg: 'bg-red-50' },
};

const TYPE_ORDER: AgentType[] = ['core', 'specialist', 'integrator', 'motor', 'firefighter'];

function fmtCompact(n: number | undefined): string {
  if (n == null) return '0';
  if (n >= 1000) return (n / 1000).toFixed(n >= 10000 ? 0 : 1) + 'k';
  return String(n);
}

export function ModulesPanel({ data }: Props) {
  const [editDoc, setEditDoc] = useState<{ name: DocName; content: string } | null>(null);
  const [createAgentOpen, setCreateAgentOpen] = useState(false);
  const [docsOpen, setDocsOpen] = useState(true);
  const [agentsOpen, setAgentsOpen] = useState(true);

  const agentCounts = useMemo(() => {
    const byType: Partial<Record<AgentType, number>> = {};
    if (!data?.agents?.manifest) return { total: 0, byType };
    const agents = data.agents.manifest.agents;
    let total = 0;
    for (const name of Object.keys(agents)) {
      const t = (agents[name].type as AgentType) || 'specialist';
      byType[t] = (byType[t] || 0) + 1;
      total++;
    }
    return { total, byType };
  }, [data]);

  if (!data) {
    return (
      <div className="flex h-32 items-center justify-center text-sm text-muted-foreground">
        carregando…
      </div>
    );
  }

  const stats = data.stats || {};
  const docs: Array<{ name: DocName; content: string | null | undefined }> = [
    { name: 'SOUL.md', content: data.soulMd },
    { name: 'PROJECT.md', content: data.projectMd },
    { name: 'DECISIONS.md', content: data.decisionsMd },
    { name: 'GOTCHAS.md', content: data.gotchasMd },
    { name: 'GLOSSARY.md', content: data.glossaryMd },
    { name: 'CODE_INDEX.md', content: data.codeIndexMd },
  ];
  const docsPresent = docs.filter((d) => !!d.content).length;

  return (
    <div className="space-y-4 pb-4">
      <div className="rounded-xl border bg-white p-3.5 shadow-sm">
        <div className="flex items-center gap-3">
          <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-primary/10">
            <FolderGit2 className="h-4.5 w-4.5 text-primary" />
          </div>
          <div className="min-w-0 flex-1">
            <h2 className="truncate text-sm font-semibold leading-tight">
              {data.project.name}
            </h2>
            <p
              className="truncate font-mono text-[10px] text-muted-foreground"
              title={data.project.root}
            >
              {data.project.root.replace(/^.*\//, '…/')}
            </p>
          </div>
        </div>
      </div>

      <div>
        <SectionLabel>Memória</SectionLabel>
        <div className="grid grid-cols-2 gap-2">
          <StatCard
            icon={<FileText className="h-4 w-4" />}
            tone="indigo"
            label="sessões"
            value={stats.sessions}
          />
          <StatCard
            icon={<ScrollText className="h-4 w-4" />}
            tone="sky"
            label="observações"
            value={stats.observations}
          />
          <StatCard
            icon={<Database className="h-4 w-4" />}
            tone="emerald"
            label="decisões"
            value={stats.decisions}
          />
          <StatCard
            icon={<AlertTriangle className="h-4 w-4" />}
            tone="amber"
            label="gotchas"
            value={stats.gotchas}
          />
        </div>
      </div>

      <Section
        icon={<BookText className="h-4 w-4" />}
        label="Documentos"
        badge={`${docsPresent}/${docs.length}`}
        open={docsOpen}
        onToggle={() => setDocsOpen((o) => !o)}
      >
        <div className="space-y-1">
          {docs.map((d) => (
            <DocRow
              key={d.name}
              label={d.name}
              hint={DOC_HINTS[d.name]}
              present={!!d.content}
              onOpen={() => setEditDoc({ name: d.name, content: d.content || '' })}
            />
          ))}
        </div>
      </Section>

      <Section
        icon={<Boxes className="h-4 w-4" />}
        label="Agentes"
        badge={agentCounts.total.toString()}
        open={agentsOpen}
        onToggle={() => setAgentsOpen((o) => !o)}
        action={
          <button
            onClick={(e) => {
              e.stopPropagation();
              setCreateAgentOpen(true);
            }}
            className="flex items-center gap-1 rounded-md border border-indigo-200 bg-indigo-50 px-2 py-1 text-[11px] font-medium text-indigo-700 transition-colors hover:bg-indigo-100"
            title="criar novo agente"
          >
            <Plus className="h-3 w-3" /> novo
          </button>
        }
      >
        <div className="space-y-1">
          {TYPE_ORDER.filter((t) => (agentCounts.byType[t] || 0) > 0).map((t) => (
            <TypeRow
              key={t}
              type={t}
              count={agentCounts.byType[t] || 0}
              total={agentCounts.total}
            />
          ))}
        </div>
      </Section>

      {stats.codeIndex && (
        <div className="rounded-xl border bg-white p-3 shadow-sm">
          <div className="mb-2 flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
            <Code2 className="h-3.5 w-3.5" /> Code Index
          </div>
          <div className="flex items-baseline justify-between">
            <div>
              <div className="text-lg font-semibold tabular-nums leading-none">
                {fmtCompact(stats.codeIndex.files)}
              </div>
              <div className="mt-0.5 text-[10px] uppercase tracking-wide text-muted-foreground">
                arquivos
              </div>
            </div>
            <div className="h-8 w-px bg-border" />
            <div>
              <div className="text-lg font-semibold tabular-nums leading-none">
                {fmtCompact(stats.codeIndex.symbols)}
              </div>
              <div className="mt-0.5 text-[10px] uppercase tracking-wide text-muted-foreground">
                símbolos
              </div>
            </div>
          </div>
        </div>
      )}

      {editDoc && (
        <DocEditor
          name={editDoc.name}
          current={editDoc.content}
          hint={DOC_HINTS[editDoc.name]}
          onClose={() => setEditDoc(null)}
        />
      )}

      {createAgentOpen && (
        <CreateAgentModal
          existingNames={data.agents?.manifest ? Object.keys(data.agents.manifest.agents) : []}
          onClose={() => setCreateAgentOpen(false)}
        />
      )}
    </div>
  );
}

function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <h3 className="mb-2 px-1 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
      {children}
    </h3>
  );
}

function Section({
  icon,
  label,
  badge,
  open,
  onToggle,
  action,
  children,
}: {
  icon: React.ReactNode;
  label: string;
  badge?: string;
  open: boolean;
  onToggle: () => void;
  action?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <div className="overflow-hidden rounded-xl border bg-white shadow-sm">
      <div
        className="flex cursor-pointer items-center gap-2 px-3 py-2.5 transition-colors hover:bg-slate-50/70"
        onClick={onToggle}
      >
        <span className="text-muted-foreground">{icon}</span>
        <h3 className="flex-1 text-xs font-semibold uppercase tracking-wide">
          {label}
        </h3>
        {badge != null && (
          <span className="rounded-full bg-muted px-2 py-0.5 font-mono text-[10px] text-muted-foreground">
            {badge}
          </span>
        )}
        {action}
        <ChevronDown
          className={cn(
            'h-3.5 w-3.5 text-muted-foreground transition-transform',
            !open && '-rotate-90',
          )}
        />
      </div>
      {open && <div className="border-t bg-slate-50/30 p-2">{children}</div>}
    </div>
  );
}

function StatCard({
  icon,
  tone,
  label,
  value,
}: {
  icon: React.ReactNode;
  tone: 'indigo' | 'sky' | 'emerald' | 'amber';
  label: string;
  value: number | undefined;
}) {
  const toneMap: Record<string, { bg: string; fg: string }> = {
    indigo: { bg: 'bg-indigo-50', fg: 'text-indigo-700' },
    sky: { bg: 'bg-sky-50', fg: 'text-sky-700' },
    emerald: { bg: 'bg-emerald-50', fg: 'text-emerald-700' },
    amber: { bg: 'bg-amber-50', fg: 'text-amber-700' },
  };
  const t = toneMap[tone];
  return (
    <div className="rounded-xl border bg-white p-3 shadow-sm transition-shadow hover:shadow-md">
      <div className="flex items-center justify-between">
        <div className={cn('flex h-7 w-7 items-center justify-center rounded-lg', t.bg, t.fg)}>
          {icon}
        </div>
      </div>
      <div className="mt-2 text-xl font-semibold tabular-nums leading-none">
        {fmtCompact(value ?? 0)}
      </div>
      <div className="mt-1 text-[10px] uppercase tracking-wide text-muted-foreground">
        {label}
      </div>
    </div>
  );
}

function DocRow({
  label,
  hint,
  present,
  onOpen,
}: {
  label: string;
  hint: string;
  present: boolean;
  onOpen: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onOpen}
      className="group flex w-full items-center gap-2.5 rounded-lg bg-white px-2.5 py-2 text-left transition-colors hover:bg-indigo-50/50"
      title={present ? 'abrir pra editar' : 'criar e editar'}
    >
      <span className="shrink-0">
        {present ? (
          <CircleCheck className="h-4 w-4 text-emerald-500" />
        ) : (
          <CircleDashed className="h-4 w-4 text-slate-300" />
        )}
      </span>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-1.5">
          <span className="truncate font-mono text-[12px] font-medium text-slate-800 group-hover:text-indigo-700">
            {label}
          </span>
        </div>
        <p className="truncate text-[10px] text-muted-foreground">{hint}</p>
      </div>
      <Pencil className="h-3.5 w-3.5 shrink-0 text-slate-400 opacity-0 transition-opacity group-hover:opacity-100" />
    </button>
  );
}

function TypeRow({
  type,
  count,
  total,
}: {
  type: AgentType;
  count: number;
  total: number;
}) {
  const pct = total > 0 ? (count / total) * 100 : 0;
  const meta = TYPE_META[type];
  return (
    <div className="group rounded-lg bg-white px-2.5 py-2 transition-colors hover:bg-slate-50">
      <div className="flex items-center gap-2">
        <span
          className="inline-block h-2.5 w-2.5 shrink-0 rounded-full ring-2"
          style={{ background: meta.color, boxShadow: `0 0 0 2px ${meta.color}20` }}
          aria-hidden
        />
        <span className="flex-1 truncate text-[12px] font-medium text-slate-800">
          {meta.label}
        </span>
        <span className="font-mono text-[11px] tabular-nums text-slate-600">
          {count}
        </span>
      </div>
      <div className="mt-1.5 h-1 w-full overflow-hidden rounded-full bg-muted">
        <span
          className="block h-full rounded-full transition-all"
          style={{ width: `${pct}%`, background: meta.color, opacity: 0.75 }}
        />
      </div>
    </div>
  );
}

interface DocEditorProps {
  name: DocName;
  current: string;
  hint: string;
  onClose: () => void;
}

function DocEditor({ name, current, hint, onClose }: DocEditorProps) {
  const [content, setContent] = useState<string>(current);
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const dirty = content !== current;

  const save = async () => {
    setErr(null);
    setSaving(true);
    try {
      await saveDoc(name, content);
      onClose();
    } catch (e: any) {
      setErr(e?.message || 'falha ao salvar');
    } finally {
      setSaving(false);
    }
  };

  const tryClose = () => {
    if (saving) return;
    if (dirty && !confirm(`Descartar alterações em ${name}?`)) return;
    onClose();
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/50 backdrop-blur-sm p-4">
      <div className="flex max-h-[90vh] w-full max-w-3xl flex-col overflow-hidden rounded-xl border bg-white shadow-2xl">
        <header className="flex items-center justify-between border-b px-4 py-3">
          <div>
            <div className="flex items-center gap-2 text-sm font-semibold">
              <FileText className="h-4 w-4" /> {name}
            </div>
            <p className="text-[11px] text-muted-foreground">{hint}</p>
          </div>
          <Button size="icon" variant="ghost" className="h-7 w-7" onClick={tryClose}>
            <X className="h-4 w-4" />
          </Button>
        </header>
        <div className="flex-1 overflow-hidden p-4">
          <textarea
            value={content}
            onChange={(e) => setContent(e.target.value)}
            spellCheck={false}
            className="h-[60vh] w-full resize-none rounded border border-slate-200 bg-white p-3 font-mono text-[11px] leading-snug focus:border-indigo-400 focus:outline-none"
            placeholder={current ? '' : `# ${name.replace('.md', '')}\n\n...`}
          />
          <div className="mt-2 flex items-center justify-between text-[10px] text-slate-500">
            <span>{content.length} chars · markdown</span>
            {dirty && <span className="text-amber-600">alterações não salvas</span>}
          </div>
          {err && (
            <div className="mt-2 rounded border border-red-200 bg-red-50 px-2 py-1.5 text-[11px] text-red-700">
              {err}
            </div>
          )}
        </div>
        <footer className="flex justify-end gap-2 border-t px-4 py-3">
          <Button size="sm" variant="outline" onClick={tryClose} disabled={saving}>
            cancelar
          </Button>
          <Button size="sm" onClick={save} disabled={saving || !dirty} className="gap-1.5">
            <Save className="h-3.5 w-3.5" />
            {saving ? 'salvando…' : 'salvar'}
          </Button>
        </footer>
      </div>
    </div>
  );
}
