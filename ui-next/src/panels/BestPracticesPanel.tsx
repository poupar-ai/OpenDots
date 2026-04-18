import { useEffect, useState } from 'react';
import { X, BookOpen, Check, Download, Plus, Save, Sparkles, Pencil, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import {
  listBestPractices,
  getBestPractice,
  applyBestPractices,
  createBestPractice,
  updateBestPractice,
  deleteBestPractice,
  type BestPractice,
} from '@/lib/bestPractices';

interface Props {
  onClose: () => void;
}

export function BestPracticesPanel({ onClose }: Props) {
  const [items, setItems] = useState<BestPractice[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [content, setContent] = useState<string>('');
  const [installing, setInstalling] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [newOpen, setNewOpen] = useState(false);
  const [editTarget, setEditTarget] = useState<BestPractice | null>(null);
  const [deleting, setDeleting] = useState<string | null>(null);

  const reload = async () => {
    try {
      const j = await listBestPractices();
      setItems(j.available);
    } catch (e: any) {
      setErr(e?.message || 'falha ao listar');
    }
  };

  useEffect(() => { reload(); }, []);

  useEffect(() => {
    if (!selected) { setContent(''); return; }
    getBestPractice(selected)
      .then((j) => setContent(j.content))
      .catch((e) => setErr(e?.message || 'falha ao ler'));
  }, [selected]);

  const install = async (id: string) => {
    setInstalling(id);
    try {
      await applyBestPractices([id], false);
      await reload();
    } catch (e: any) {
      setErr(e?.message || 'falha ao instalar');
    } finally {
      setInstalling(null);
    }
  };

  const remove = async (id: string) => {
    if (!confirm(`Apagar best-practice "${id}"? Essa ação não pode ser desfeita.`)) return;
    setDeleting(id);
    try {
      await deleteBestPractice(id);
      if (selected === id) setSelected(null);
      await reload();
    } catch (e: any) {
      setErr(e?.message || 'falha ao apagar');
    } finally {
      setDeleting(null);
    }
  };

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-slate-900/50 backdrop-blur-sm p-4">
      <div className="flex h-[85vh] w-full max-w-5xl flex-col overflow-hidden rounded-xl border bg-white shadow-2xl">
        <header className="flex items-center justify-between border-b px-4 py-3">
          <div>
            <div className="flex items-center gap-2 text-sm font-semibold">
              <BookOpen className="h-4 w-4" /> Best-practices
            </div>
            <p className="text-xs text-muted-foreground">
              conhecimento de domínio injetável nos agentes, versionado por projeto
            </p>
          </div>
          <div className="flex items-center gap-2">
            <Button size="sm" variant="outline" className="gap-1.5" onClick={() => setNewOpen(true)}>
              <Plus className="h-3.5 w-3.5" /> nova
            </Button>
            <Button size="icon" variant="ghost" className="h-7 w-7" onClick={onClose}>
              <X className="h-4 w-4" />
            </Button>
          </div>
        </header>

        <div className="grid flex-1 grid-cols-[280px_1fr] overflow-hidden">
          <aside className="soft-scroll overflow-y-auto border-r p-3">
            {err && (
              <div className="mb-2 rounded border border-red-200 bg-red-50 px-2 py-1.5 text-xs text-red-700">{err}</div>
            )}
            {!items && <p className="text-xs text-muted-foreground">carregando…</p>}
            {items?.map((p) => (
              <button
                key={p.id}
                onClick={() => setSelected(p.id)}
                className={`mb-1.5 block w-full rounded border px-2 py-2 text-left text-xs transition ${
                  selected === p.id
                    ? 'border-primary/50 bg-primary/5'
                    : 'border-slate-200 hover:bg-slate-50'
                }`}
              >
                <div className="flex items-center justify-between gap-2">
                  <span className="font-semibold">{p.name}</span>
                  <span className="flex items-center gap-1">
                    {p.source === 'local' && (
                      <span className="rounded bg-amber-100 px-1 py-0.5 text-xs font-semibold uppercase text-amber-700">local</span>
                    )}
                    {p.installed && <Check className="h-3 w-3 text-emerald-600" />}
                  </span>
                </div>
                <p className="mt-0.5 text-xs text-slate-500">{p.scope}</p>
                {p.tags && p.tags.length > 0 && (
                  <div className="mt-1 flex flex-wrap gap-1">
                    {p.tags.slice(0, 4).map((t) => (
                      <span key={t} className="rounded bg-slate-100 px-1 py-0.5 text-xs text-slate-600">
                        {t}
                      </span>
                    ))}
                  </div>
                )}
              </button>
            ))}
          </aside>

          <main className="soft-scroll overflow-y-auto p-4">
            {!selected && (
              <p className="py-10 text-center text-xs text-slate-500">
                Selecione uma prática na lista.
              </p>
            )}
            {selected && items && (
              (() => {
                const p = items.find(x => x.id === selected);
                if (!p) return null;
                return (
                  <div>
                    <div className="mb-3 flex items-start justify-between gap-3">
                      <div>
                        <h2 className="text-base font-semibold">{p.name}</h2>
                        <p className="text-xs text-slate-500">
                          {p.scope}
                          {p.tags && ` · ${p.tags.join(', ')}`}
                        </p>
                        {p.when_to_use && (
                          <p className="mt-1 whitespace-pre-line text-xs text-slate-700">
                            <strong>Quando usar:</strong> {p.when_to_use}
                          </p>
                        )}
                        {p.not_for && (
                          <p className="mt-0.5 text-xs text-slate-500">
                            <strong>Não usar para:</strong> {p.not_for}
                          </p>
                        )}
                      </div>
                      <div className="flex shrink-0 items-center gap-1.5">
                        {p.source === 'local' && (
                          <>
                            <Button
                              size="sm"
                              variant="outline"
                              className="gap-1.5"
                              onClick={() => setEditTarget(p)}
                              title="editar metadata e conteúdo"
                            >
                              <Pencil className="h-3.5 w-3.5" /> editar
                            </Button>
                            <Button
                              size="sm"
                              variant="outline"
                              className="gap-1.5 border-red-200 text-red-700 hover:bg-red-50"
                              onClick={() => remove(p.id)}
                              disabled={deleting === p.id}
                              title="apagar best-practice local"
                            >
                              <Trash2 className="h-3.5 w-3.5" />
                              {deleting === p.id ? 'apagando…' : 'apagar'}
                            </Button>
                          </>
                        )}
                        {!p.installed ? (
                          <Button
                            size="sm"
                            onClick={() => install(p.id)}
                            disabled={installing === p.id}
                            className="gap-1.5"
                          >
                            <Download className="h-3.5 w-3.5" />
                            {installing === p.id ? 'instalando…' : 'instalar'}
                          </Button>
                        ) : (
                          <Badge variant="outline">
                            <Check className="h-3 w-3" /> instalado
                          </Badge>
                        )}
                      </div>
                    </div>
                    <pre className="whitespace-pre-wrap rounded bg-slate-50 p-3 font-mono text-xs leading-relaxed text-slate-800">
                      {content || '(sem conteúdo)'}
                    </pre>
                  </div>
                );
              })()
            )}
          </main>
        </div>
      </div>
      {newOpen && (
        <BestPracticeForm
          mode="create"
          existingIds={(items || []).map(p => p.id)}
          onClose={() => setNewOpen(false)}
          onSaved={async (id) => {
            setNewOpen(false);
            await reload();
            setSelected(id);
          }}
        />
      )}
      {editTarget && (
        <BestPracticeForm
          mode="edit"
          existingIds={(items || []).map(p => p.id)}
          initial={{
            id: editTarget.id,
            name: editTarget.name,
            scope: editTarget.scope || '',
            tags: editTarget.tags || [],
            when_to_use: editTarget.when_to_use || '',
            not_for: editTarget.not_for || '',
            content,
          }}
          onClose={() => setEditTarget(null)}
          onSaved={async (id) => {
            setEditTarget(null);
            await reload();
            setSelected(id);
            try {
              const j = await getBestPractice(id);
              setContent(j.content);
            } catch {}
          }}
        />
      )}
    </div>
  );
}

interface FormProps {
  mode: 'create' | 'edit';
  existingIds: string[];
  initial?: {
    id: string;
    name: string;
    scope: string;
    tags: string[];
    when_to_use: string;
    not_for: string;
    content: string;
  };
  onClose: () => void;
  onSaved: (id: string) => void;
}

function slugify(s: string): string {
  return s
    .toLowerCase()
    .normalize('NFD')
    .replace(/[\u0300-\u036f]/g, '')
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 50);
}

function BestPracticeForm({ mode, existingIds, initial, onClose, onSaved }: FormProps) {
  const isEdit = mode === 'edit';
  const [name, setName] = useState(initial?.name || '');
  const [id, setId] = useState(initial?.id || '');
  const [idEdited, setIdEdited] = useState(isEdit);
  const [scope, setScope] = useState(initial?.scope || '');
  const [tagsText, setTagsText] = useState((initial?.tags || []).join(', '));
  const [whenToUse, setWhenToUse] = useState(initial?.when_to_use || '');
  const [notFor, setNotFor] = useState(initial?.not_for || '');
  const [content, setContent] = useState(initial?.content || '');
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const effectiveId = isEdit ? (initial?.id || '') : (idEdited ? id : slugify(name));
  const idTaken = !isEdit && existingIds.includes(effectiveId);
  const idValid = /^[a-z][a-z0-9-]{1,48}[a-z0-9]$/.test(effectiveId);
  const canSave = !!name.trim() && !!content.trim() && idValid && !idTaken && !saving;

  const save = async () => {
    setErr(null);
    setSaving(true);
    try {
      const tags = tagsText.split(',').map(t => t.trim()).filter(Boolean);
      if (isEdit) {
        await updateBestPractice(effectiveId, {
          name: name.trim(),
          scope: scope.trim(),
          tags,
          when_to_use: whenToUse.trim(),
          not_for: notFor.trim(),
          content,
        });
      } else {
        await createBestPractice({
          id: effectiveId,
          name: name.trim(),
          scope: scope.trim() || undefined,
          tags: tags.length ? tags : undefined,
          when_to_use: whenToUse.trim() || undefined,
          not_for: notFor.trim() || undefined,
          content,
        });
      }
      onSaved(effectiveId);
    } catch (e: any) {
      setErr(e?.message || `falha ao ${isEdit ? 'salvar' : 'criar'}`);
    } finally {
      setSaving(false);
    }
  };

  const dirty = isEdit
    ? (name !== (initial?.name || '')
        || content !== (initial?.content || '')
        || scope !== (initial?.scope || '')
        || whenToUse !== (initial?.when_to_use || '')
        || notFor !== (initial?.not_for || '')
        || tagsText !== (initial?.tags || []).join(', '))
    : !!(name || content || scope || whenToUse || notFor || tagsText);

  const tryClose = () => {
    if (saving) return;
    if (dirty && !confirm(isEdit ? 'Descartar alterações?' : 'Descartar nova best-practice?')) return;
    onClose();
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/60 backdrop-blur-sm p-4">
      <div className="flex max-h-[90vh] w-full max-w-3xl flex-col overflow-hidden rounded-xl border bg-white shadow-2xl">
        <header className="flex items-center justify-between border-b px-4 py-3">
          <div>
            <div className="flex items-center gap-2 text-sm font-semibold">
              {isEdit ? <Pencil className="h-4 w-4" /> : <Sparkles className="h-4 w-4" />}
              {isEdit ? `editar ${effectiveId}` : 'nova best-practice'}
            </div>
            <p className="text-xs text-muted-foreground">
              fica em <code className="font-mono">.claude-project-memory/best-practices/&lt;id&gt;/</code> — versionada junto com o projeto
            </p>
          </div>
          <Button size="icon" variant="ghost" className="h-7 w-7" onClick={tryClose}>
            <X className="h-4 w-4" />
          </Button>
        </header>

        <div className="soft-scroll flex-1 overflow-y-auto p-4 text-xs">
          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="mb-1 block text-xs font-semibold uppercase text-slate-600">nome</label>
              <input
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="Ex: Padrões de migrations Alembic"
                className="w-full rounded border border-slate-200 bg-white px-2 py-1.5 text-xs focus:border-indigo-400 focus:outline-none"
              />
            </div>
            <div>
              <label className="mb-1 block text-xs font-semibold uppercase text-slate-600">id (slug)</label>
              <input
                value={effectiveId}
                readOnly={isEdit}
                onChange={(e) => { if (!isEdit) { setId(e.target.value); setIdEdited(true); } }}
                placeholder="migrations-alembic"
                className={`w-full rounded border bg-white px-2 py-1.5 font-mono text-xs focus:outline-none ${
                  isEdit ? 'border-slate-200 bg-slate-50 text-slate-500' :
                  effectiveId && !idValid ? 'border-red-300 focus:border-red-400' : idTaken ? 'border-amber-300 focus:border-amber-400' : 'border-slate-200 focus:border-indigo-400'
                }`}
              />
              {isEdit ? (
                <p className="mt-0.5 text-xs text-slate-400">slug não pode mudar depois de criada</p>
              ) : (
                <>
                  {effectiveId && !idValid && (
                    <p className="mt-0.5 text-xs text-red-600">slug inválido (só letras minúsculas, dígitos, hífen)</p>
                  )}
                  {idValid && idTaken && (
                    <p className="mt-0.5 text-xs text-amber-600">já existe uma best-practice com esse id</p>
                  )}
                </>
              )}
            </div>
          </div>

          <div className="mt-3 grid grid-cols-2 gap-3">
            <div>
              <label className="mb-1 block text-xs font-semibold uppercase text-slate-600">scope</label>
              <input
                value={scope}
                onChange={(e) => setScope(e.target.value)}
                placeholder="backend / frontend / infra / …"
                className="w-full rounded border border-slate-200 bg-white px-2 py-1.5 text-xs focus:border-indigo-400 focus:outline-none"
              />
            </div>
            <div>
              <label className="mb-1 block text-xs font-semibold uppercase text-slate-600">tags (vírgulas)</label>
              <input
                value={tagsText}
                onChange={(e) => setTagsText(e.target.value)}
                placeholder="sql, migrations, postgres"
                className="w-full rounded border border-slate-200 bg-white px-2 py-1.5 text-xs focus:border-indigo-400 focus:outline-none"
              />
            </div>
          </div>

          <div className="mt-3">
            <label className="mb-1 block text-xs font-semibold uppercase text-slate-600">quando usar</label>
            <input
              value={whenToUse}
              onChange={(e) => setWhenToUse(e.target.value)}
              placeholder="quando for criar ou revisar uma migration Alembic"
              className="w-full rounded border border-slate-200 bg-white px-2 py-1.5 text-xs focus:border-indigo-400 focus:outline-none"
            />
          </div>

          <div className="mt-3">
            <label className="mb-1 block text-xs font-semibold uppercase text-slate-600">não usar para</label>
            <input
              value={notFor}
              onChange={(e) => setNotFor(e.target.value)}
              placeholder="dados transacionais que precisam de raw SQL"
              className="w-full rounded border border-slate-200 bg-white px-2 py-1.5 text-xs focus:border-indigo-400 focus:outline-none"
            />
          </div>

          <div className="mt-3">
            <label className="mb-1 block text-xs font-semibold uppercase text-slate-600">conteúdo (markdown)</label>
            <textarea
              value={content}
              onChange={(e) => setContent(e.target.value)}
              spellCheck={false}
              placeholder={'# Migrations Alembic\n\n## Regras\n- nunca alterar migration já aplicada\n- ...'}
              className="h-64 w-full resize-y rounded border border-slate-200 bg-white p-2 font-mono text-xs leading-snug focus:border-indigo-400 focus:outline-none"
            />
            <p className="mt-0.5 text-xs text-slate-500">{content.length} chars</p>
          </div>

          {err && (
            <div className="mt-3 rounded border border-red-200 bg-red-50 px-2 py-1.5 text-xs text-red-700">
              {err}
            </div>
          )}
        </div>

        <footer className="flex justify-end gap-2 border-t px-4 py-3">
          <Button size="sm" variant="outline" onClick={tryClose} disabled={saving}>
            cancelar
          </Button>
          <Button size="sm" onClick={save} disabled={!canSave || (isEdit && !dirty)} className="gap-1.5">
            <Save className="h-3.5 w-3.5" />
            {saving ? (isEdit ? 'salvando…' : 'criando…') : (isEdit ? 'salvar' : 'criar')}
          </Button>
        </footer>
      </div>
    </div>
  );
}
