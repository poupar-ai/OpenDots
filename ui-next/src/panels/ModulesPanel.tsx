import { useMemo, useState } from 'react';
import {
  FileText, Database, AlertTriangle, ScrollText, Save, Pencil, Plus,
  FolderGit2, BookText, Boxes, Code2, ChevronDown, CircleCheck, CircleDashed,
} from 'lucide-react';
import { Badge, Button, NavLink, Paper, Text, colorVar } from '@/ui';
import type { Color } from '@/ui';
import { cn } from '@/lib/utils';
import type { ServerData, AgentType } from '@/lib/types';
import { saveDoc, type DocName } from '@/lib/docs';
import { DocEditor } from '@/panels/DocEditor';
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

const TYPE_META: Record<AgentType, { label: string; color: string }> = {
  core:        { label: 'Core',         color: '#6366f1' },
  specialist:  { label: 'Specialists',  color: '#0ea5e9' },
  integrator:  { label: 'Integrators',  color: '#a855f7' },
  motor:       { label: 'Motors',       color: '#f97316' },
  firefighter: { label: 'Firefighters', color: '#ef4444' },
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
      <div className="flex h-32 items-center justify-center">
        <Text size="sm" muted>carregando…</Text>
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
      <Paper padding="sm" radius="lg" withBorder shadow="xs">
        <div className="flex items-center gap-3">
          <span
            className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg"
            style={{ background: colorVar('primary', 1), color: colorVar('primary', 6) }}
          >
            <FolderGit2 className="h-[18px] w-[18px]" />
          </span>
          <div className="min-w-0 flex-1">
            <Text size="sm" fw={600}>{data.project.name}</Text>
            <p className="truncate font-mono text-xs text-muted-foreground" title={data.project.root}>
              {data.project.root.replace(/^.*\//, '…/')}
            </p>
          </div>
        </div>
      </Paper>

      <div>
        <SectionLabel>Memória</SectionLabel>
        <div className="grid grid-cols-2 gap-2">
          <StatCard icon={<FileText className="h-4 w-4" />} tone="primary" label="sessões" value={stats.sessions} />
          <StatCard icon={<ScrollText className="h-4 w-4" />} tone="info" label="observações" value={stats.observations} />
          <StatCard icon={<Database className="h-4 w-4" />} tone="success" label="decisões" value={stats.decisions} />
          <StatCard icon={<AlertTriangle className="h-4 w-4" />} tone="warning" label="gotchas" value={stats.gotchas} />
        </div>
      </div>

      <Section
        icon={<BookText className="h-4 w-4" />}
        label="Documentos"
        badge={`${docsPresent}/${docs.length}`}
        open={docsOpen}
        onToggle={() => setDocsOpen((o) => !o)}
      >
        <div className="space-y-0.5">
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
          <span onClick={(e) => e.stopPropagation()}>
            <Button
              size="compact-xs"
              variant="light"
              color="primary"
              leftIcon={<Plus className="h-3 w-3" />}
              onClick={() => setCreateAgentOpen(true)}
              title="criar novo agente"
            >
              novo
            </Button>
          </span>
        }
      >
        <div className="space-y-1">
          {TYPE_ORDER.filter((t) => (agentCounts.byType[t] || 0) > 0).map((t) => (
            <TypeRow key={t} type={t} count={agentCounts.byType[t] || 0} total={agentCounts.total} />
          ))}
        </div>
      </Section>

      {stats.codeIndex && (
        <Paper padding="sm" radius="lg" withBorder shadow="xs">
          <div className="mb-2 flex items-center gap-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
            <Code2 className="h-3.5 w-3.5" /> Code Index
          </div>
          <div className="flex items-baseline justify-between">
            <div>
              <div className="text-lg font-semibold tabular-nums leading-none">{fmtCompact(stats.codeIndex.files)}</div>
              <div className="mt-0.5 text-xs uppercase tracking-wide text-muted-foreground">arquivos</div>
            </div>
            <div className="h-8 w-px bg-border" />
            <div>
              <div className="text-lg font-semibold tabular-nums leading-none">{fmtCompact(stats.codeIndex.symbols)}</div>
              <div className="mt-0.5 text-xs uppercase tracking-wide text-muted-foreground">símbolos</div>
            </div>
          </div>
        </Paper>
      )}

      {editDoc && (
        <DocEditor
          name={editDoc.name}
          current={editDoc.content}
          hint={DOC_HINTS[editDoc.name]}
          onSave={saveDoc}
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
    <h3 className="mb-2 px-1 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
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
    <Paper padding={0} radius="lg" withBorder shadow="xs" className="overflow-hidden">
      <div
        className="flex cursor-pointer items-center gap-2 px-3 py-2.5 transition-colors hover:bg-slate-50/70"
        onClick={onToggle}
      >
        <span className="text-muted-foreground">{icon}</span>
        <h3 className="flex-1 text-xs font-semibold uppercase tracking-wide">{label}</h3>
        {badge != null && <Badge color="gray" variant="light" size="sm">{badge}</Badge>}
        {action}
        <ChevronDown className={cn('h-3.5 w-3.5 text-muted-foreground transition-transform', !open && '-rotate-90')} />
      </div>
      {open && <div className="border-t bg-slate-50/30 p-2">{children}</div>}
    </Paper>
  );
}

function StatCard({
  icon,
  tone,
  label,
  value,
}: {
  icon: React.ReactNode;
  tone: Color;
  label: string;
  value: number | undefined;
}) {
  return (
    <Paper padding="sm" radius="lg" withBorder shadow="xs" className="transition-shadow hover:shadow-md">
      <span
        className="flex h-7 w-7 items-center justify-center rounded-lg"
        style={{ background: colorVar(tone, 1), color: colorVar(tone, 7) }}
      >
        {icon}
      </span>
      <div className="mt-2 text-xl font-semibold tabular-nums leading-none">{fmtCompact(value ?? 0)}</div>
      <div className="mt-1 text-xs uppercase tracking-wide text-muted-foreground">{label}</div>
    </Paper>
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
    <NavLink
      label={<span className="font-mono text-xs font-medium">{label}</span>}
      description={hint}
      onClick={onOpen}
      leftIcon={
        present ? (
          <CircleCheck className="h-4 w-4 text-emerald-500" />
        ) : (
          <CircleDashed className="h-4 w-4 text-slate-300" />
        )
      }
      rightSection={<Pencil className="h-3.5 w-3.5 text-slate-400" />}
    />
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
        <span className="flex-1 truncate text-xs font-medium text-slate-800">{meta.label}</span>
        <span className="font-mono text-xs tabular-nums text-slate-600">{count}</span>
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
