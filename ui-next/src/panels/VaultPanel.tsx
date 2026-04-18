import { useEffect, useMemo, useState } from 'react';
import { X, KeyRound, Lock, Unlock, Plus, Trash2, Save, Eye, AlertTriangle, FileText, Copy, Check, Pencil, Download, Upload, HardDrive } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import {
  getVaultStatus,
  initVault,
  unlockVault,
  lockVault,
  listVaultKeys,
  getVaultSecret,
  setVaultSecret,
  deleteVaultSecret,
  listEnvFiles,
  getEnvFile,
  setEnvFile,
  deleteEnvFile,
  destroyVault,
  changeVaultPassword,
  getVaultPaths,
  exportVault,
  importVault,
  generateBundlePassword,
  type VaultStatus,
  type EnvFileEntry,
  type VaultPaths,
} from '@/lib/vault';

interface Props {
  onClose: () => void;
}

function fmtTime(ts: number): string {
  const d = new Date(ts);
  return d.toLocaleTimeString('pt-BR', { hour: '2-digit', minute: '2-digit' });
}

type Tab = 'secrets' | 'envfiles';

export function VaultPanel({ onClose }: Props) {
  const [status, setStatus] = useState<VaultStatus | null>(null);
  const [keys, setKeys] = useState<string[]>([]);
  const [envfiles, setEnvfiles] = useState<EnvFileEntry[]>([]);
  const [tab, setTab] = useState<Tab>('secrets');
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [copiedName, setCopiedName] = useState<string | null>(null);

  // Forms
  const [newPass, setNewPass] = useState('');
  const [confirmPass, setConfirmPass] = useState('');
  const [unlockPass, setUnlockPass] = useState('');
  const [ttlMin, setTtlMin] = useState(30);

  // Secret editor
  const [editing, setEditing] = useState<{ name: string; value: string; isNew: boolean } | null>(null);

  // Envfile editor
  const [editingEnv, setEditingEnv] = useState<{ name: string; content: string; isNew: boolean } | null>(null);

  // Change password form
  const [pwChange, setPwChange] = useState<{ old: string; next: string; confirm: string } | null>(null);

  // Export/import
  const [paths, setPaths] = useState<VaultPaths | null>(null);
  const [pathCopied, setPathCopied] = useState(false);
  const [exportForm, setExportForm] = useState<{
    master: string;
    useRandom: boolean;
    bundle: string;
    copied: boolean;
    manualBundle: string;
  } | null>(null);
  const [importForm, setImportForm] = useState<{
    file: File | null;
    pass: string;
    newMaster: string;
    confirmMaster: string;
    replace: boolean;
  } | null>(null);

  const refresh = async () => {
    try {
      const s = await getVaultStatus();
      setStatus(s);
      if (s.initialized && !s.locked) {
        const [ks, efs] = await Promise.all([listVaultKeys(), listEnvFiles()]);
        setKeys(ks);
        setEnvfiles(efs);
      } else {
        setKeys([]);
        setEnvfiles([]);
      }
    } catch (e: any) {
      setErr(e?.message || 'falha ao ler status do vault');
    }
  };

  useEffect(() => {
    refresh();
    // Poll status a cada 10s pra capturar expiração de sessão
    const id = setInterval(refresh, 10000);
    return () => clearInterval(id);
  }, []);

  useEffect(() => {
    getVaultPaths().then(setPaths).catch(() => {});
  }, []);

  const expiresLabel = useMemo(() => {
    if (!status?.expires_at) return null;
    return fmtTime(status.expires_at);
  }, [status]);

  const handleInit = async () => {
    setErr(null);
    if (newPass.length < 8) return setErr('senha mínima de 8 caracteres');
    if (newPass !== confirmPass) return setErr('senhas não coincidem');
    setBusy(true);
    try {
      await initVault(newPass);
      setNewPass(''); setConfirmPass('');
      await refresh();
    } catch (e: any) {
      setErr(e?.message || 'falha ao criar vault');
    } finally { setBusy(false); }
  };

  const handleUnlock = async () => {
    setErr(null);
    if (!unlockPass) return setErr('digite a senha');
    setBusy(true);
    try {
      await unlockVault(unlockPass, ttlMin);
      setUnlockPass('');
      await refresh();
    } catch (e: any) {
      setErr(e?.message || 'senha inválida');
    } finally { setBusy(false); }
  };

  const handleLock = async () => {
    setBusy(true);
    try { await lockVault(); await refresh(); }
    finally { setBusy(false); }
  };

  const handleSaveSecret = async () => {
    if (!editing) return;
    setErr(null);
    if (!/^[A-Z][A-Z0-9_]{0,63}$/.test(editing.name)) {
      return setErr('nome deve seguir padrão [A-Z][A-Z0-9_]{0,63} (ex: ASAAS_TOKEN)');
    }
    if (!editing.value.trim()) return setErr('valor não pode ser vazio');
    setBusy(true);
    try {
      await setVaultSecret(editing.name, editing.value);
      setEditing(null);
      await refresh();
    } catch (e: any) {
      setErr(e?.message || 'falha ao salvar');
    } finally { setBusy(false); }
  };

  const handleDeleteSecret = async (name: string) => {
    if (!confirm(`Apagar secret ${name}?`)) return;
    setBusy(true);
    try { await deleteVaultSecret(name); await refresh(); }
    catch (e: any) { setErr(e?.message || 'falha ao apagar'); }
    finally { setBusy(false); }
  };

  const handleReveal = async (name: string) => {
    try {
      const v = await getVaultSecret(name);
      setEditing({ name, value: v, isNew: false });
    } catch (e: any) { setErr(e?.message || 'falha ao ler secret'); }
  };

  // ── Envfiles ──────────────────────────────────────────────────────────────
  const handleEditEnv = async (name: string) => {
    try {
      const content = await getEnvFile(name);
      setEditingEnv({ name, content, isNew: false });
    } catch (e: any) { setErr(e?.message || 'falha ao ler envfile'); }
  };

  const handleSaveEnv = async () => {
    if (!editingEnv) return;
    setErr(null);
    if (!/^[A-Za-z0-9._\-\/]{1,120}$/.test(editingEnv.name)) {
      return setErr('nome inválido (aceito: letras, números, . _ - /). Ex: apps/api/.env.local');
    }
    if (!editingEnv.content.trim()) return setErr('conteúdo vazio');
    setBusy(true);
    try {
      await setEnvFile(editingEnv.name, editingEnv.content);
      setEditingEnv(null);
      await refresh();
    } catch (e: any) {
      setErr(e?.message || 'falha ao salvar envfile');
    } finally { setBusy(false); }
  };

  const handleDeleteEnv = async (name: string) => {
    if (!confirm(`Apagar envfile ${name}?`)) return;
    setBusy(true);
    try { await deleteEnvFile(name); await refresh(); }
    catch (e: any) { setErr(e?.message || 'falha ao apagar'); }
    finally { setBusy(false); }
  };

  const handleCopyEnv = async (name: string) => {
    try {
      const content = await getEnvFile(name);
      await navigator.clipboard.writeText(content);
      setCopiedName(name);
      setTimeout(() => setCopiedName((cur) => (cur === name ? null : cur)), 1500);
    } catch (e: any) { setErr(e?.message || 'falha ao copiar'); }
  };

  const handleChangePassword = async () => {
    if (!pwChange) return;
    setErr(null);
    if (pwChange.next.length < 8) return setErr('nova senha mínima de 8 caracteres');
    if (pwChange.next !== pwChange.confirm) return setErr('senhas não coincidem');
    setBusy(true);
    try {
      await changeVaultPassword(pwChange.old, pwChange.next);
      setPwChange(null);
    } catch (e: any) {
      setErr(e?.message || 'falha ao trocar senha');
    } finally { setBusy(false); }
  };

  const handleCopyPath = async () => {
    if (!paths) return;
    try {
      await navigator.clipboard.writeText(paths.mem_dir);
      setPathCopied(true);
      setTimeout(() => setPathCopied(false), 1500);
    } catch (e: any) { setErr(e?.message || 'falha ao copiar'); }
  };

  const handleToggleRandomBundle = (useRandom: boolean) => {
    if (!exportForm) return;
    if (useRandom) {
      setExportForm({ ...exportForm, useRandom: true, bundle: generateBundlePassword(), copied: false });
    } else {
      setExportForm({ ...exportForm, useRandom: false, bundle: '', copied: false });
    }
  };

  const handleRegenBundle = () => {
    if (!exportForm) return;
    setExportForm({ ...exportForm, bundle: generateBundlePassword(), copied: false });
  };

  const handleCopyBundle = async () => {
    if (!exportForm?.bundle) return;
    try {
      await navigator.clipboard.writeText(exportForm.bundle);
      setExportForm({ ...exportForm, copied: true });
      setTimeout(() => {
        setExportForm((cur) => (cur ? { ...cur, copied: false } : cur));
      }, 1500);
    } catch (e: any) { setErr(e?.message || 'falha ao copiar'); }
  };

  const handleExport = async () => {
    if (!exportForm) return;
    setErr(null);
    if (!exportForm.master) return setErr('digite a senha mestra');
    const bundlePw = exportForm.useRandom
      ? exportForm.bundle
      : exportForm.manualBundle.trim();
    if (exportForm.useRandom && !exportForm.copied) {
      if (!confirm('Você não copiou a senha gerada. Se perder, o bundle vira inútil. Continuar mesmo assim?')) return;
    }
    setBusy(true);
    try {
      const blob = await exportVault(exportForm.master, bundlePw || undefined);
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `vault-${new Date().toISOString().slice(0, 10)}.pmvexp`;
      document.body.appendChild(a);
      a.click();
      a.remove();
      URL.revokeObjectURL(url);
      setExportForm(null);
    } catch (e: any) {
      setErr(e?.message || 'falha ao exportar');
    } finally { setBusy(false); }
  };

  const handleImport = async () => {
    if (!importForm?.file) return setErr('selecione o arquivo do bundle');
    if (!importForm.pass) return setErr('digite a senha do bundle');
    if (importForm.newMaster.length < 8) return setErr('nova senha mestra mínima de 8 caracteres');
    if (importForm.newMaster !== importForm.confirmMaster) return setErr('confirmação da senha mestra não bate');
    const currentInitialized = !!status?.initialized;
    if (currentInitialized && !importForm.replace) {
      if (!confirm('Já existe um cofre aqui. Importar vai SUBSTITUIR tudo — confirma?')) return;
    }
    setErr(null);
    setBusy(true);
    try {
      await importVault(importForm.file, importForm.pass, importForm.newMaster, currentInitialized);
      setImportForm(null);
      await refresh();
    } catch (e: any) {
      setErr(e?.message || 'falha ao importar');
    } finally { setBusy(false); }
  };

  const handleDestroy = async () => {
    const input = prompt('DANGER: isso apaga todo o cofre. Digite DESTROY pra confirmar:');
    if (input !== 'DESTROY') return;
    setBusy(true);
    try { await destroyVault(); await refresh(); }
    catch (e: any) { setErr(e?.message || 'falha ao destruir'); }
    finally { setBusy(false); }
  };

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-slate-900/50 backdrop-blur-sm p-4">
      <div className="flex h-[85vh] w-full max-w-3xl flex-col overflow-hidden rounded-xl border bg-white shadow-2xl">
        <header className="flex items-start justify-between gap-4 border-b px-4 py-3">
          <div className="flex-1">
            <div className="flex items-center gap-2 text-sm font-semibold">
              <KeyRound className="h-4 w-4 text-amber-600" /> Cofre — credenciais do project-mem
            </div>
            <p className="mt-0.5 text-xs text-muted-foreground">
              Cofre encriptado <strong>local</strong> (AES-256-GCM). Guarda credenciais que os <strong>agentes do project-mem</strong> precisam pra chamar serviços externos (tokens, chaves, webhooks). As credenciais que sua <strong>aplicação</strong> usa em runtime continuam nos arquivos <code className="rounded bg-slate-100 px-1 text-[10px]">.env</code> do seu projeto — o cofre só guarda <em>cópias/backup</em> deles na aba "arquivos .env".
            </p>
            <p className="mt-1 text-[11px] text-muted-foreground">
              <strong>Modelo de acesso:</strong> agentes autorizados no manifest (<code>secrets: [...]</code>) acessam <em>sempre</em>, sem depender de senha. A <strong>senha mestra</strong> só protege esta UI — humano precisa dela pra ver/editar/apagar. Auto-lock só tranca a UI, agentes seguem trabalhando.
            </p>
          </div>
          <Button size="icon" variant="ghost" onClick={onClose}><X className="h-4 w-4" /></Button>
        </header>

        {err && (
          <div className="mx-4 mt-3 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-700">
            {err}
          </div>
        )}

        <div className="flex-1 overflow-y-auto p-4">
          {!status && <p className="text-sm text-slate-500">carregando…</p>}

          {status && !status.initialized && (
            <div className="mx-auto max-w-md space-y-3">
              <div className="rounded-lg border border-amber-200 bg-amber-50 p-3 text-xs text-amber-800">
                <div className="flex items-center gap-1.5 font-semibold">
                  <AlertTriangle className="h-3.5 w-3.5" /> Primeiro uso
                </div>
                <p className="mt-1">
                  Crie uma senha mestra pro cofre. <strong>Não existe recuperação</strong> — se perder, perde todos os secrets. Se já tem um bundle <code className="rounded bg-white px-1">.pmvexp</code> de outra máquina, clique em <strong>importar</strong> abaixo.
                </p>
              </div>
              {paths && (
                <div className="rounded-md border border-slate-200 bg-slate-50 px-3 py-2 text-[10px] text-slate-600">
                  <div className="flex items-center gap-1.5"><HardDrive className="h-3 w-3" /> Cofre ficará em:</div>
                  <code className="mt-1 block break-all font-mono text-[10px] text-slate-700">{paths.mem_dir}</code>
                </div>
              )}
              <label className="block text-sm font-semibold text-slate-700">Nova senha (mín. 8)</label>
              <input
                type="password"
                value={newPass}
                onChange={(e) => setNewPass(e.target.value)}
                className="w-full rounded-md border border-slate-300 bg-white px-3 py-2 text-sm text-slate-900 placeholder:text-slate-400 focus:border-indigo-400 focus:outline-none"
                placeholder="senha mestra"
                autoComplete="new-password"
              />
              <label className="block text-sm font-semibold text-slate-700">Confirmar senha</label>
              <input
                type="password"
                value={confirmPass}
                onChange={(e) => setConfirmPass(e.target.value)}
                className="w-full rounded-md border border-slate-300 bg-white px-3 py-2 text-sm text-slate-900 placeholder:text-slate-400 focus:border-indigo-400 focus:outline-none"
                placeholder="de novo"
                autoComplete="new-password"
              />
              <Button onClick={handleInit} disabled={busy} className="w-full">
                {busy ? 'criando…' : 'criar cofre'}
              </Button>
              <div className="border-t pt-3">
                <Button
                  onClick={() => setImportForm({ file: null, pass: '', newMaster: '', confirmMaster: '', replace: false })}
                  disabled={busy || !!importForm}
                  variant="outline"
                  size="sm"
                  className="w-full"
                >
                  <Upload className="mr-1.5 h-3 w-3" /> importar bundle de outra máquina
                </Button>
                {importForm && (
                  <div className="mt-2 rounded-lg border border-slate-200 bg-slate-50 p-3 space-y-2">
                    <p className="text-[11px] text-slate-600">
                      A <strong>senha do bundle</strong> é a que quem gerou o arquivo compartilhou contigo. A <strong>nova senha mestra</strong> é sua — só você precisa saber pra entrar nesta UI.
                    </p>
                    <input
                      type="file"
                      accept=".pmvexp"
                      onChange={(e) => setImportForm({ ...importForm, file: e.target.files?.[0] || null })}
                      className="w-full rounded-md border border-slate-300 bg-white px-2.5 py-1.5 text-xs text-slate-900 focus:border-indigo-400 focus:outline-none"
                    />
                    <input
                      type="password"
                      value={importForm.pass}
                      onChange={(e) => setImportForm({ ...importForm, pass: e.target.value })}
                      placeholder="senha do bundle (recebida)"
                      className="w-full rounded-md border border-slate-300 bg-white px-2.5 py-1.5 text-sm text-slate-900 focus:border-indigo-400 focus:outline-none"
                      autoComplete="current-password"
                    />
                    <input
                      type="password"
                      value={importForm.newMaster}
                      onChange={(e) => setImportForm({ ...importForm, newMaster: e.target.value })}
                      placeholder="nova senha mestra da UI (mín. 8)"
                      className="w-full rounded-md border border-slate-300 bg-white px-2.5 py-1.5 text-sm text-slate-900 focus:border-indigo-400 focus:outline-none"
                      autoComplete="new-password"
                    />
                    <input
                      type="password"
                      value={importForm.confirmMaster}
                      onChange={(e) => setImportForm({ ...importForm, confirmMaster: e.target.value })}
                      placeholder="confirmar nova senha mestra"
                      className="w-full rounded-md border border-slate-300 bg-white px-2.5 py-1.5 text-sm text-slate-900 focus:border-indigo-400 focus:outline-none"
                      autoComplete="new-password"
                    />
                    <div className="flex gap-2">
                      <Button size="sm" onClick={handleImport} disabled={busy || !importForm.file || !importForm.pass || !importForm.newMaster}>
                        <Upload className="mr-1.5 h-3 w-3" /> importar
                      </Button>
                      <Button size="sm" variant="outline" onClick={() => setImportForm(null)}>
                        cancelar
                      </Button>
                    </div>
                  </div>
                )}
              </div>
            </div>
          )}

          {status && status.initialized && status.locked && (
            <div className="mx-auto max-w-md space-y-3">
              <div className="flex items-center gap-2 rounded-lg border bg-slate-50 p-3">
                <Lock className="h-4 w-4 text-slate-500" />
                <span className="text-sm font-medium text-slate-700">UI travada — agentes seguem funcionando</span>
              </div>
              {status.needs_migration && (
                <div className="rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-xs text-amber-800">
                  <strong>Cofre no formato antigo.</strong> Digite a senha pra migrar pro novo modelo (device-key). É transparente e preserva todos os secrets/.env.
                </div>
              )}
              <label className="block text-sm font-semibold text-slate-700">Senha</label>
              <input
                type="password"
                value={unlockPass}
                onChange={(e) => setUnlockPass(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && handleUnlock()}
                className="w-full rounded-md border border-slate-300 bg-white px-3 py-2 text-sm text-slate-900 placeholder:text-slate-400 focus:border-indigo-400 focus:outline-none"
                placeholder="senha mestra"
                autoComplete="current-password"
                autoFocus
              />
              <div className="flex items-center gap-2">
                <label className="text-xs font-medium text-slate-600">auto-travar UI em:</label>
                <select
                  value={ttlMin}
                  onChange={(e) => setTtlMin(parseInt(e.target.value, 10))}
                  className="rounded-md border border-slate-300 bg-white px-2 py-1 text-xs text-slate-900 focus:border-indigo-400 focus:outline-none"
                >
                  <option value={5}>5 min</option>
                  <option value={15}>15 min</option>
                  <option value={30}>30 min</option>
                  <option value={60}>1h</option>
                  <option value={120}>2h</option>
                  <option value={240}>4h</option>
                </select>
              </div>
              <Button onClick={handleUnlock} disabled={busy} className="w-full">
                <Unlock className="mr-2 h-3.5 w-3.5" />
                {busy ? 'abrindo…' : 'desbloquear'}
              </Button>
              <div className="mt-6 border-t pt-3 text-[10px] text-slate-500">
                Esqueceu a senha? Delete os arquivos <code className="rounded bg-slate-100 px-1">vault.enc</code> e <code className="rounded bg-slate-100 px-1">.vault-devicekey</code> do diretório de memória do projeto. Não existe reset remoto.
              </div>
            </div>
          )}

          {status && status.initialized && !status.locked && (
            <div className="space-y-4">
              <div className="flex items-center justify-between rounded-lg border border-emerald-200 bg-emerald-50 px-3 py-2">
                <div className="flex items-center gap-2 text-sm text-emerald-800">
                  <Unlock className="h-4 w-4" />
                  <span className="font-medium">UI desbloqueada</span>
                  {expiresLabel && <Badge variant="outline" className="ml-1 text-[10px]">trava às {expiresLabel}</Badge>}
                </div>
                <Button size="sm" variant="outline" onClick={handleLock} disabled={busy}>
                  <Lock className="mr-1.5 h-3 w-3" /> travar UI
                </Button>
              </div>

              {paths && (
                <div className="rounded-md border border-slate-200 bg-slate-50 px-3 py-2 text-[11px] text-slate-700">
                  <div className="flex items-center gap-1.5 font-semibold text-slate-800">
                    <HardDrive className="h-3 w-3" /> Localização física
                  </div>
                  <div className="mt-1 flex items-center gap-2">
                    <code className="flex-1 truncate rounded bg-white px-2 py-1 font-mono text-[10px] text-slate-700 border border-slate-200" title={paths.mem_dir}>
                      {paths.mem_dir}
                    </code>
                    <Button size="icon" variant="ghost" className="h-6 w-6" onClick={handleCopyPath} title="copiar path">
                      {pathCopied ? <Check className="h-3 w-3 text-emerald-600" /> : <Copy className="h-3 w-3" />}
                    </Button>
                  </div>
                  <p className="mt-1.5 text-slate-500">
                    Arquivos: <code className="rounded bg-white px-1">vault.enc</code> + <code className="rounded bg-white px-1">.vault-devicekey</code>. Pra mover pra outra máquina, use <strong>exportar cofre</strong> abaixo.
                  </p>
                </div>
              )}

              <div className="flex gap-1 border-b">
                <button
                  onClick={() => setTab('secrets')}
                  className={`px-3 py-1.5 text-sm font-medium transition ${tab === 'secrets' ? 'border-b-2 border-primary text-slate-900' : 'text-slate-500 hover:text-slate-700'}`}
                >
                  <KeyRound className="mr-1.5 inline h-3.5 w-3.5" /> secrets ({keys.length})
                </button>
                <button
                  onClick={() => setTab('envfiles')}
                  className={`px-3 py-1.5 text-sm font-medium transition ${tab === 'envfiles' ? 'border-b-2 border-primary text-slate-900' : 'text-slate-500 hover:text-slate-700'}`}
                >
                  <FileText className="mr-1.5 inline h-3.5 w-3.5" /> arquivos .env ({envfiles.length})
                </button>
              </div>

              {tab === 'secrets' && (
                <>
                  <div className="rounded-md border border-indigo-100 bg-indigo-50/40 px-3 py-2 text-[11px] text-slate-600">
                    <strong>Pros agentes do project-mem</strong> chamarem APIs externas. Cada secret é injetado como env var no subprocess do agente que declarar o nome em <code className="rounded bg-white px-1">secrets: [...]</code> no manifest — vale como autorização automática, não precisa de senha a cada invocação.
                  </div>

                  <div className="flex items-center justify-between">
                    <h3 className="text-sm font-semibold text-slate-700">Secrets</h3>
                    <Button
                      size="sm"
                      onClick={() => setEditing({ name: '', value: '', isNew: true })}
                      disabled={!!editing}
                    >
                      <Plus className="mr-1.5 h-3 w-3" /> novo secret
                    </Button>
                  </div>

                  {editing && (
                    <div className="rounded-lg border border-indigo-200 bg-indigo-50/50 p-3 space-y-2">
                      <div>
                        <label className="block text-xs font-semibold text-slate-700">Nome (UPPER_SNAKE)</label>
                        <input
                          type="text"
                          value={editing.name}
                          disabled={!editing.isNew}
                          onChange={(e) => setEditing({ ...editing, name: e.target.value.toUpperCase() })}
                          placeholder="ASAAS_TOKEN"
                          className="mt-1 w-full rounded-md border border-slate-300 bg-white px-2.5 py-1.5 text-sm font-mono text-slate-900 placeholder:text-slate-400 focus:border-indigo-400 focus:outline-none disabled:bg-slate-100"
                        />
                      </div>
                      <div>
                        <label className="block text-xs font-semibold text-slate-700">Valor</label>
                        <textarea
                          value={editing.value}
                          onChange={(e) => setEditing({ ...editing, value: e.target.value })}
                          placeholder="sk_test_..."
                          rows={3}
                          className="mt-1 w-full rounded-md border border-slate-300 bg-white px-2.5 py-1.5 text-sm font-mono text-slate-900 placeholder:text-slate-400 focus:border-indigo-400 focus:outline-none"
                        />
                      </div>
                      <div className="flex gap-2">
                        <Button size="sm" onClick={handleSaveSecret} disabled={busy}>
                          <Save className="mr-1.5 h-3 w-3" /> salvar
                        </Button>
                        <Button size="sm" variant="outline" onClick={() => setEditing(null)}>
                          cancelar
                        </Button>
                      </div>
                    </div>
                  )}

                  {keys.length === 0 ? (
                    <p className="text-xs text-slate-500">nenhum secret ainda — clique em "novo secret" pra começar.</p>
                  ) : (
                    <ul className="space-y-1">
                      {keys.map((k) => (
                        <li key={k} className="flex items-center justify-between rounded-md border bg-white px-3 py-2 hover:bg-slate-50">
                          <code className="text-sm font-mono text-slate-900">{k}</code>
                          <div className="flex items-center gap-1">
                            <Button
                              size="icon"
                              variant="ghost"
                              className="h-7 w-7"
                              onClick={() => handleReveal(k)}
                              title="ver / editar valor"
                            >
                              <Eye className="h-3.5 w-3.5" />
                            </Button>
                            <Button
                              size="icon"
                              variant="ghost"
                              className="h-7 w-7 text-red-600 hover:bg-red-50"
                              onClick={() => handleDeleteSecret(k)}
                              title="apagar"
                            >
                              <Trash2 className="h-3.5 w-3.5" />
                            </Button>
                          </div>
                        </li>
                      ))}
                    </ul>
                  )}

                  <div className="mt-6 rounded-lg border border-slate-200 bg-slate-50 p-3 text-xs text-slate-600">
                    <div className="font-semibold text-slate-700">Como usar nos agentes</div>
                    <p className="mt-1">No manifest do agente, declare:</p>
                    <pre className="mt-1.5 rounded bg-white p-2 text-[11px] font-mono text-slate-800">{`secrets: [ASAAS_TOKEN, OPENAI_KEY]`}</pre>
                    <p className="mt-1.5">
                      No spawn do agente, essas envs são injetadas automaticamente — a UI pode estar travada que não afeta. Se algum nome não existir no cofre, agente roda sem aquela env e loga <code>vault_partial</code>.
                    </p>
                  </div>
                </>
              )}

              {tab === 'envfiles' && (
                <>
                  <div className="rounded-md border border-amber-100 bg-amber-50/60 px-3 py-2 text-[11px] text-slate-600">
                    <strong>Backup dos .env do seu projeto</strong> — guarde cópias aqui pra portabilidade entre máquinas ou compartilhar com o time. A aplicação em runtime continua lendo dos arquivos <code className="rounded bg-white px-1">.env</code> do disco. Daqui você pode <strong>copiar o conteúdo inteiro</strong> e colar onde precisar.
                  </div>

                  <div className="flex items-center justify-between">
                    <h3 className="text-sm font-semibold text-slate-700">Arquivos .env</h3>
                    <Button
                      size="sm"
                      onClick={() => setEditingEnv({ name: '', content: '', isNew: true })}
                      disabled={!!editingEnv}
                    >
                      <Plus className="mr-1.5 h-3 w-3" /> novo .env
                    </Button>
                  </div>

                  {editingEnv && (
                    <div className="rounded-lg border border-amber-200 bg-amber-50/50 p-3 space-y-2">
                      <div>
                        <label className="block text-xs font-semibold text-slate-700">Nome / caminho</label>
                        <input
                          type="text"
                          value={editingEnv.name}
                          disabled={!editingEnv.isNew}
                          onChange={(e) => setEditingEnv({ ...editingEnv, name: e.target.value })}
                          placeholder="apps/api/.env.local"
                          className="mt-1 w-full rounded-md border border-slate-300 bg-white px-2.5 py-1.5 text-sm font-mono text-slate-900 placeholder:text-slate-400 focus:border-indigo-400 focus:outline-none disabled:bg-slate-100"
                        />
                        <p className="mt-1 text-[10px] text-slate-500">use um caminho ou tag (ex: <code>prod</code>, <code>apps/api/.env.local</code>)</p>
                      </div>
                      <div>
                        <label className="block text-xs font-semibold text-slate-700">Conteúdo do .env</label>
                        <textarea
                          value={editingEnv.content}
                          onChange={(e) => setEditingEnv({ ...editingEnv, content: e.target.value })}
                          placeholder={'DATABASE_URL=postgres://...\nAPI_KEY=...\n'}
                          rows={10}
                          className="mt-1 w-full rounded-md border border-slate-300 bg-white px-2.5 py-1.5 text-xs font-mono text-slate-900 placeholder:text-slate-400 focus:border-indigo-400 focus:outline-none"
                          spellCheck={false}
                        />
                      </div>
                      <div className="flex gap-2">
                        <Button size="sm" onClick={handleSaveEnv} disabled={busy}>
                          <Save className="mr-1.5 h-3 w-3" /> salvar
                        </Button>
                        <Button size="sm" variant="outline" onClick={() => setEditingEnv(null)}>
                          cancelar
                        </Button>
                      </div>
                    </div>
                  )}

                  {envfiles.length === 0 ? (
                    <p className="text-xs text-slate-500">nenhum .env salvo — clique em "novo .env" pra guardar um backup.</p>
                  ) : (
                    <ul className="space-y-1">
                      {envfiles.map((f) => (
                        <li key={f.name} className="flex items-center justify-between rounded-md border bg-white px-3 py-2 hover:bg-slate-50">
                          <div className="min-w-0 flex-1">
                            <code className="text-sm font-mono text-slate-900">{f.name}</code>
                            <div className="mt-0.5 flex items-center gap-2 text-[10px] text-slate-500">
                              <span>{f.size} bytes</span>
                              {f.updated_at && <span>· {new Date(f.updated_at).toLocaleString('pt-BR')}</span>}
                            </div>
                          </div>
                          <div className="flex items-center gap-1">
                            <Button
                              size="icon"
                              variant="ghost"
                              className="h-7 w-7"
                              onClick={() => handleCopyEnv(f.name)}
                              title="copiar .env inteiro pro clipboard"
                            >
                              {copiedName === f.name ? (
                                <Check className="h-3.5 w-3.5 text-emerald-600" />
                              ) : (
                                <Copy className="h-3.5 w-3.5" />
                              )}
                            </Button>
                            <Button
                              size="icon"
                              variant="ghost"
                              className="h-7 w-7"
                              onClick={() => handleEditEnv(f.name)}
                              title="ver / editar"
                            >
                              <Eye className="h-3.5 w-3.5" />
                            </Button>
                            <Button
                              size="icon"
                              variant="ghost"
                              className="h-7 w-7 text-red-600 hover:bg-red-50"
                              onClick={() => handleDeleteEnv(f.name)}
                              title="apagar"
                            >
                              <Trash2 className="h-3.5 w-3.5" />
                            </Button>
                          </div>
                        </li>
                      ))}
                    </ul>
                  )}
                </>
              )}

              <div className="mt-4 border-t pt-3 space-y-2">
                <div className="flex flex-wrap gap-2">
                  <Button
                    onClick={() => setPwChange({ old: '', next: '', confirm: '' })}
                    disabled={busy || !!pwChange}
                    variant="outline"
                    size="sm"
                  >
                    <Pencil className="mr-1.5 h-3 w-3" /> trocar senha
                  </Button>
                  <Button
                    onClick={() => setExportForm({ master: '', useRandom: true, bundle: generateBundlePassword(), copied: false, manualBundle: '' })}
                    disabled={busy || exportForm !== null}
                    variant="outline"
                    size="sm"
                  >
                    <Download className="mr-1.5 h-3 w-3" /> exportar cofre
                  </Button>
                  <Button
                    onClick={() => setImportForm({ file: null, pass: '', newMaster: '', confirmMaster: '', replace: true })}
                    disabled={busy || !!importForm}
                    variant="outline"
                    size="sm"
                  >
                    <Upload className="mr-1.5 h-3 w-3" /> importar cofre
                  </Button>
                  <Button
                    onClick={handleDestroy}
                    disabled={busy}
                    variant="outline"
                    size="sm"
                    className="border-red-200 text-red-600 hover:bg-red-50"
                  >
                    <Trash2 className="mr-1.5 h-3 w-3" /> destruir cofre
                  </Button>
                </div>

                {exportForm !== null && (
                  <div className="rounded-lg border border-slate-200 bg-slate-50 p-3 space-y-3">
                    <p className="text-[11px] text-slate-600">
                      Exporta um bundle criptografado (<code>.pmvexp</code>) com vault + device-key. O arquivo fica inútil sem a senha do bundle — guarde-a separada do arquivo.
                    </p>

                    <div>
                      <label className="block text-xs font-semibold text-slate-700">Senha mestra (autoriza o export)</label>
                      <input
                        type="password"
                        value={exportForm.master}
                        onChange={(e) => setExportForm({ ...exportForm, master: e.target.value })}
                        placeholder="senha mestra do cofre"
                        className="mt-1 w-full rounded-md border border-slate-300 bg-white px-2.5 py-1.5 text-sm text-slate-900 focus:border-indigo-400 focus:outline-none"
                        autoComplete="current-password"
                      />
                    </div>

                    <div className="rounded-md border border-slate-200 bg-white p-2.5 space-y-2">
                      <label className="flex items-center gap-2 text-xs font-semibold text-slate-700">
                        <input
                          type="checkbox"
                          checked={exportForm.useRandom}
                          onChange={(e) => handleToggleRandomBundle(e.target.checked)}
                          className="h-3.5 w-3.5"
                        />
                        usar senha aleatória para o bundle (recomendado)
                      </label>

                      {exportForm.useRandom ? (
                        <>
                          <div className="flex items-center gap-1.5">
                            <code className="flex-1 truncate rounded border border-slate-200 bg-slate-50 px-2 py-1.5 font-mono text-[11px] text-slate-900" title={exportForm.bundle}>
                              {exportForm.bundle}
                            </code>
                            <Button size="icon" variant="ghost" className="h-7 w-7" onClick={handleCopyBundle} title="copiar senha do bundle">
                              {exportForm.copied ? <Check className="h-3.5 w-3.5 text-emerald-600" /> : <Copy className="h-3.5 w-3.5" />}
                            </Button>
                            <Button size="icon" variant="ghost" className="h-7 w-7" onClick={handleRegenBundle} title="gerar nova">
                              <KeyRound className="h-3.5 w-3.5" />
                            </Button>
                          </div>
                          <div className="flex items-start gap-1.5 rounded-md border border-red-200 bg-red-50 px-2 py-1.5 text-[11px] text-red-700">
                            <AlertTriangle className="mt-0.5 h-3 w-3 shrink-0" />
                            <span>
                              <strong>Anote agora</strong> em um gerenciador de senha. Se perder essa senha, o bundle <strong>não</strong> pode ser restaurado — nem por nós, nem por você.
                            </span>
                          </div>
                        </>
                      ) : (
                        <div>
                          <label className="block text-[11px] text-slate-600">
                            Senha do bundle (deixe vazio pra reusar a senha mestra)
                          </label>
                          <input
                            type="text"
                            value={exportForm.manualBundle}
                            onChange={(e) => setExportForm({ ...exportForm, manualBundle: e.target.value })}
                            placeholder="senha do bundle (opcional)"
                            className="mt-1 w-full rounded-md border border-slate-300 bg-white px-2.5 py-1.5 text-sm font-mono text-slate-900 focus:border-indigo-400 focus:outline-none"
                            autoComplete="off"
                          />
                          <p className="mt-1 text-[10px] text-slate-500">
                            Se vazio: bundle encripta com a <strong>mesma senha mestra</strong> acima (menos seguro pra backup em nuvem).
                          </p>
                        </div>
                      )}
                    </div>

                    <div className="flex gap-2">
                      <Button size="sm" onClick={handleExport} disabled={busy || !exportForm.master}>
                        <Download className="mr-1.5 h-3 w-3" /> baixar bundle
                      </Button>
                      <Button size="sm" variant="outline" onClick={() => setExportForm(null)}>
                        cancelar
                      </Button>
                    </div>
                  </div>
                )}

                {importForm && (
                  <div className="rounded-lg border border-amber-200 bg-amber-50 p-3 space-y-2">
                    <p className="text-[11px] text-amber-800">
                      <strong>Atenção:</strong> importar vai <strong>substituir</strong> o cofre atual. Os secrets e .envs daqui serão perdidos.
                    </p>
                    <p className="text-[11px] text-slate-600">
                      A <strong>senha do bundle</strong> vem de quem te enviou o arquivo. A <strong>nova senha mestra</strong> é sua — independe da senha de quem exportou.
                    </p>
                    <input
                      type="file"
                      accept=".pmvexp"
                      onChange={(e) => setImportForm({ ...importForm, file: e.target.files?.[0] || null })}
                      className="w-full rounded-md border border-slate-300 bg-white px-2.5 py-1.5 text-xs text-slate-900 focus:border-indigo-400 focus:outline-none"
                    />
                    <input
                      type="password"
                      value={importForm.pass}
                      onChange={(e) => setImportForm({ ...importForm, pass: e.target.value })}
                      placeholder="senha do bundle (recebida)"
                      className="w-full rounded-md border border-slate-300 bg-white px-2.5 py-1.5 text-sm text-slate-900 focus:border-indigo-400 focus:outline-none"
                      autoComplete="current-password"
                    />
                    <input
                      type="password"
                      value={importForm.newMaster}
                      onChange={(e) => setImportForm({ ...importForm, newMaster: e.target.value })}
                      placeholder="nova senha mestra da UI (mín. 8)"
                      className="w-full rounded-md border border-slate-300 bg-white px-2.5 py-1.5 text-sm text-slate-900 focus:border-indigo-400 focus:outline-none"
                      autoComplete="new-password"
                    />
                    <input
                      type="password"
                      value={importForm.confirmMaster}
                      onChange={(e) => setImportForm({ ...importForm, confirmMaster: e.target.value })}
                      placeholder="confirmar nova senha mestra"
                      className="w-full rounded-md border border-slate-300 bg-white px-2.5 py-1.5 text-sm text-slate-900 focus:border-indigo-400 focus:outline-none"
                      autoComplete="new-password"
                    />
                    <div className="flex gap-2">
                      <Button size="sm" onClick={handleImport} disabled={busy || !importForm.file || !importForm.pass || !importForm.newMaster}>
                        <Upload className="mr-1.5 h-3 w-3" /> substituir cofre
                      </Button>
                      <Button size="sm" variant="outline" onClick={() => setImportForm(null)}>
                        cancelar
                      </Button>
                    </div>
                  </div>
                )}
                {pwChange && (
                  <div className="rounded-lg border border-slate-200 bg-slate-50 p-3 space-y-2">
                    <label className="block text-xs font-semibold text-slate-700">Senha atual</label>
                    <input
                      type="password"
                      value={pwChange.old}
                      onChange={(e) => setPwChange({ ...pwChange, old: e.target.value })}
                      className="w-full rounded-md border border-slate-300 bg-white px-2.5 py-1.5 text-sm text-slate-900 focus:border-indigo-400 focus:outline-none"
                      autoComplete="current-password"
                    />
                    <label className="block text-xs font-semibold text-slate-700">Nova senha (mín. 8)</label>
                    <input
                      type="password"
                      value={pwChange.next}
                      onChange={(e) => setPwChange({ ...pwChange, next: e.target.value })}
                      className="w-full rounded-md border border-slate-300 bg-white px-2.5 py-1.5 text-sm text-slate-900 focus:border-indigo-400 focus:outline-none"
                      autoComplete="new-password"
                    />
                    <label className="block text-xs font-semibold text-slate-700">Confirmar nova</label>
                    <input
                      type="password"
                      value={pwChange.confirm}
                      onChange={(e) => setPwChange({ ...pwChange, confirm: e.target.value })}
                      className="w-full rounded-md border border-slate-300 bg-white px-2.5 py-1.5 text-sm text-slate-900 focus:border-indigo-400 focus:outline-none"
                      autoComplete="new-password"
                    />
                    <div className="flex gap-2">
                      <Button size="sm" onClick={handleChangePassword} disabled={busy}>
                        <Save className="mr-1.5 h-3 w-3" /> trocar
                      </Button>
                      <Button size="sm" variant="outline" onClick={() => setPwChange(null)}>
                        cancelar
                      </Button>
                    </div>
                  </div>
                )}
                <p className="text-[10px] text-slate-500">destruir cofre apaga todos os secrets e envfiles — irreversível</p>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
