import { useState } from 'react';
import { FileText, Save } from 'lucide-react';
import { Button, Modal, Text } from '@/ui';
import type { DocName } from '@/lib/docs';

interface DocEditorProps {
  name: DocName;
  current: string;
  hint: string;
  onSave: (name: DocName, content: string) => Promise<unknown>;
  onClose: () => void;
}

/** Editor de documento de memória (SOUL.md, PROJECT.md, …) em Modal. */
export function DocEditor({ name, current, hint, onSave, onClose }: DocEditorProps) {
  const [content, setContent] = useState<string>(current);
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const dirty = content !== current;

  const save = async () => {
    setErr(null);
    setSaving(true);
    try {
      await onSave(name, content);
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
    <Modal
      opened
      onClose={tryClose}
      size="xl"
      title={
        <div>
          <div className="flex items-center gap-2 text-sm font-semibold">
            <FileText className="h-4 w-4" /> {name}
          </div>
          <Text size="xs" muted>{hint}</Text>
        </div>
      }
    >
      <textarea
        value={content}
        onChange={(e) => setContent(e.target.value)}
        spellCheck={false}
        className="h-[60vh] w-full resize-none rounded border border-slate-200 bg-white p-3 font-mono text-xs leading-snug focus:border-indigo-400 focus:outline-none"
        placeholder={current ? '' : `# ${name.replace('.md', '')}\n\n...`}
      />
      <div className="mt-2 flex items-center justify-between text-xs text-slate-500">
        <span>{content.length} chars · markdown</span>
        {dirty && <span className="text-amber-600">alterações não salvas</span>}
      </div>
      {err && (
        <div className="mt-2 rounded border border-red-200 bg-red-50 px-2 py-1.5 text-xs text-red-700">
          {err}
        </div>
      )}
      <div className="mt-3 flex justify-end gap-2">
        <Button size="sm" variant="default" color="gray" onClick={tryClose} disabled={saving}>
          cancelar
        </Button>
        <Button
          size="sm"
          onClick={save}
          disabled={saving || !dirty}
          leftIcon={<Save className="h-3.5 w-3.5" />}
        >
          {saving ? 'salvando…' : 'salvar'}
        </Button>
      </div>
    </Modal>
  );
}
