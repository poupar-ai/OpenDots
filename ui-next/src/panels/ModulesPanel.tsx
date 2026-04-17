import { useState } from 'react';
import { FileText, Database, AlertTriangle, ScrollText, Package, X, Save, Pencil } from 'lucide-react';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import type { ServerData } from '@/lib/types';
import { saveDoc, type DocName } from '@/lib/docs';

interface Props {
  data: ServerData | null;
}

const DOC_HINTS: Record<DocName, string> = {
  'SOUL.md': 'alma do projeto: propósito, valores, voz, anti-padrões',
  'PROJECT.md': 'visão técnica: o que é, stack, estrutura, como rodar',
  'DECISIONS.md': 'log de decisões com motivo e data',
  'GOTCHAS.md': 'armadilhas conhecidas pra quem toca o código',
  'GLOSSARY.md': 'glossário oficial dos termos do domínio',
  'CODE_INDEX.md': 'índice gerado do código — símbolos, módulos',
};

export function ModulesPanel({ data }: Props) {
  const [editDoc, setEditDoc] = useState<{ name: DocName; content: string } | null>(null);

  if (!data) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
        carregando…
      </div>
    );
  }

  const stats = data.stats || {};

  const openDoc = (name: DocName, content: string | null | undefined) => {
    setEditDoc({ name, content: content || '' });
  };

  return (
    <div className="space-y-3">
      <Card>
        <CardHeader className="pb-2">
          <div className="flex items-center justify-between">
            <CardTitle className="flex items-center gap-2">
              <Package className="h-4 w-4 text-primary" />
              {data.project.name}
            </CardTitle>
            <Badge variant="outline">project-mem</Badge>
          </div>
          <p className="font-mono text-[10px] text-muted-foreground">{data.project.root}</p>
        </CardHeader>
      </Card>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-xs uppercase tracking-wide text-muted-foreground">
            Memória
          </CardTitle>
        </CardHeader>
        <CardContent className="grid grid-cols-2 gap-2 pt-0">
          <Stat icon={<FileText className="h-3.5 w-3.5" />} label="sessões" value={stats.sessions} />
          <Stat
            icon={<ScrollText className="h-3.5 w-3.5" />}
            label="observações"
            value={stats.observations}
          />
          <Stat
            icon={<Database className="h-3.5 w-3.5" />}
            label="decisões"
            value={stats.decisions}
          />
          <Stat
            icon={<AlertTriangle className="h-3.5 w-3.5" />}
            label="gotchas"
            value={stats.gotchas}
          />
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-xs uppercase tracking-wide text-muted-foreground">
            Documentos
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-1.5 pt-0">
          <DocRow label="SOUL.md" present={!!data.soulMd} onOpen={() => openDoc('SOUL.md', data.soulMd)} />
          <DocRow label="PROJECT.md" present={!!data.projectMd} onOpen={() => openDoc('PROJECT.md', data.projectMd)} />
          <DocRow label="DECISIONS.md" present={!!data.decisionsMd} onOpen={() => openDoc('DECISIONS.md', data.decisionsMd)} />
          <DocRow label="GOTCHAS.md" present={!!data.gotchasMd} onOpen={() => openDoc('GOTCHAS.md', data.gotchasMd)} />
          <DocRow label="GLOSSARY.md" present={!!data.glossaryMd} onOpen={() => openDoc('GLOSSARY.md', data.glossaryMd)} />
          <DocRow label="CODE_INDEX.md" present={!!data.codeIndexMd} onOpen={() => openDoc('CODE_INDEX.md', data.codeIndexMd)} />
        </CardContent>
      </Card>

      {stats.codeIndex && (
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-xs uppercase tracking-wide text-muted-foreground">
              Code Index
            </CardTitle>
          </CardHeader>
          <CardContent className="pt-0">
            <div className="flex items-center justify-between text-xs">
              <span className="text-muted-foreground">arquivos</span>
              <span className="font-mono">{stats.codeIndex.files}</span>
            </div>
            <div className="flex items-center justify-between text-xs">
              <span className="text-muted-foreground">símbolos</span>
              <span className="font-mono">{stats.codeIndex.symbols}</span>
            </div>
          </CardContent>
        </Card>
      )}

      {editDoc && (
        <DocEditor
          name={editDoc.name}
          current={editDoc.content}
          hint={DOC_HINTS[editDoc.name]}
          onClose={() => setEditDoc(null)}
        />
      )}
    </div>
  );
}

function Stat({ icon, label, value }: { icon: React.ReactNode; label: string; value: number }) {
  return (
    <div className="flex items-center gap-2 rounded-md bg-muted/40 px-2 py-1.5">
      <span className="text-muted-foreground">{icon}</span>
      <div className="flex flex-col leading-tight">
        <span className="text-base font-semibold">{value ?? 0}</span>
        <span className="text-[10px] uppercase tracking-wide text-muted-foreground">{label}</span>
      </div>
    </div>
  );
}

function DocRow({ label, present, onOpen }: { label: string; present: boolean; onOpen: () => void }) {
  return (
    <button
      type="button"
      onClick={onOpen}
      className="group flex w-full items-center justify-between rounded-md px-1 py-0.5 text-xs hover:bg-indigo-50/50"
      title={present ? 'abrir pra editar' : 'criar e editar'}
    >
      <span className="flex items-center gap-1 font-mono text-slate-600 group-hover:text-indigo-700">
        {label}
        <Pencil className="h-3 w-3 opacity-0 group-hover:opacity-60" />
      </span>
      <Badge variant={present ? 'success' : 'outline'}>{present ? 'ok' : 'faltando'}</Badge>
    </button>
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
