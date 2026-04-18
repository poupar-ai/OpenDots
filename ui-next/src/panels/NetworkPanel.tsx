import { useEffect, useState } from 'react';
import { X, Network, Save, AlertTriangle, Copy, Check } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';

interface Props {
  onClose: () => void;
}

type BindMode = 'localhost' | 'lan' | 'custom';

interface LanIp {
  iface: string;
  address: string;
}

interface NetworkConfig {
  bind: BindMode;
  customHost: string;
  activeHost: string;
  port: number;
  lanIps: LanIp[];
}

export function NetworkPanel({ onClose }: Props) {
  const [cfg, setCfg] = useState<NetworkConfig | null>(null);
  const [mode, setMode] = useState<BindMode>('localhost');
  const [customHost, setCustomHost] = useState('');
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [savedMsg, setSavedMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const r = await fetch('/api/network-config');
        const j = await r.json();
        if (!alive) return;
        setCfg(j);
        setMode(j.bind);
        setCustomHost(j.customHost || '');
      } catch (e: any) {
        if (alive) setErr(e?.message || 'falha ao carregar');
      } finally {
        if (alive) setLoading(false);
      }
    })();
    return () => { alive = false; };
  }, []);

  const save = async () => {
    setSaving(true);
    setErr(null);
    setSavedMsg(null);
    try {
      const body: any = { bind: mode };
      if (mode === 'custom') body.customHost = customHost.trim();
      const r = await fetch('/api/network-config', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });
      const j = await r.json();
      if (!r.ok) throw new Error(j.error || `HTTP ${r.status}`);
      setSavedMsg('Salvo. Reinicie o servidor pra aplicar.');
    } catch (e: any) {
      setErr(e?.message || 'falha ao salvar');
    } finally {
      setSaving(false);
    }
  };

  const copy = async (url: string) => {
    try {
      await navigator.clipboard.writeText(url);
      setCopied(url);
      setTimeout(() => setCopied(null), 1500);
    } catch {}
  };

  const pendingChange = cfg
    && (mode !== cfg.bind
      || (mode === 'custom' && customHost.trim() !== (cfg.customHost || '')));

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-slate-900/50 backdrop-blur-sm p-4">
      <div className="flex max-h-[85vh] w-full max-w-xl flex-col overflow-hidden rounded-xl border bg-white shadow-2xl">
        <header className="flex items-center justify-between border-b px-4 py-3">
          <div>
            <div className="flex items-center gap-2 text-sm font-semibold">
              <Network className="h-4 w-4 text-sky-600" /> Rede — bind do servidor
            </div>
            <p className="mt-0.5 text-xs text-muted-foreground">
              Default: localhost (só este computador). Mudar pra LAN expõe a UI na rede — inclusive cofre de secrets.
            </p>
          </div>
          <Button size="icon" variant="ghost" onClick={onClose}><X className="h-4 w-4" /></Button>
        </header>

        {loading ? (
          <div className="p-4 text-xs text-muted-foreground">carregando…</div>
        ) : (
          <div className="space-y-4 overflow-y-auto p-4 text-xs">
            {cfg && (
              <div className="rounded-md border bg-slate-50 p-3">
                <div className="mb-1 font-medium text-slate-700">Ativo agora</div>
                <div className="text-slate-600">
                  bind <code className="rounded bg-white px-1 py-0.5">{cfg.activeHost}</code>
                  {' · '}
                  porta <code className="rounded bg-white px-1 py-0.5">{cfg.port}</code>
                </div>
                <div className="mt-2 space-y-1">
                  <UrlRow url={`http://localhost:${cfg.port}`} copied={copied} onCopy={copy} />
                  {cfg.lanIps.length > 0 && cfg.lanIps.map((ip) => (
                    <UrlRow
                      key={ip.address}
                      url={`http://${ip.address}:${cfg.port}`}
                      label={ip.iface}
                      copied={copied}
                      onCopy={copy}
                    />
                  ))}
                  {cfg.lanIps.length === 0 && (
                    <div className="text-muted-foreground">nenhum IP de LAN detectado</div>
                  )}
                </div>
              </div>
            )}

            <fieldset className="space-y-2">
              <legend className="mb-1 font-medium text-slate-700">Modo de bind</legend>

              <BindOption
                active={mode === 'localhost'}
                onSelect={() => setMode('localhost')}
                title="Localhost (127.0.0.1)"
                desc="Default. Só acessível deste computador. Mais seguro."
              />
              <BindOption
                active={mode === 'lan'}
                onSelect={() => setMode('lan')}
                title="LAN (0.0.0.0)"
                desc="Exposto em todas as interfaces de rede. Qualquer máquina da LAN consegue abrir."
                warning
              />
              <BindOption
                active={mode === 'custom'}
                onSelect={() => setMode('custom')}
                title="IP específico"
                desc="Exposto só no IP/host informado."
                warning
              />

              {mode === 'custom' && (
                <div className="ml-6 pt-1">
                  <Input
                    value={customHost}
                    onChange={(e) => setCustomHost(e.target.value)}
                    placeholder="ex.: 192.168.0.42"
                    className="h-8 text-xs"
                  />
                  {cfg && cfg.lanIps.length > 0 && (
                    <div className="mt-1 flex flex-wrap gap-1">
                      {cfg.lanIps.map((ip) => (
                        <button
                          key={ip.address}
                          type="button"
                          onClick={() => setCustomHost(ip.address)}
                          className="rounded border border-slate-200 bg-white px-1.5 py-0.5 text-xs hover:bg-slate-50"
                        >
                          {ip.address} <span className="text-muted-foreground">({ip.iface})</span>
                        </button>
                      ))}
                    </div>
                  )}
                </div>
              )}
            </fieldset>

            {(mode === 'lan' || mode === 'custom') && (
              <div className="flex gap-2 rounded-md border border-amber-200 bg-amber-50 p-2.5 text-amber-800">
                <AlertTriangle className="h-4 w-4 shrink-0" />
                <div>
                  <div className="font-medium">Aviso de segurança</div>
                  <div className="mt-0.5 leading-snug">
                    Expor o servidor na rede permite que outras máquinas acessem a memória do projeto e o cofre de secrets. Use apenas em redes confiáveis.
                  </div>
                </div>
              </div>
            )}

            {err && (
              <div className="rounded-md border border-red-200 bg-red-50 px-2.5 py-1.5 text-red-700">
                {err}
              </div>
            )}
            {savedMsg && (
              <div className="rounded-md border border-emerald-200 bg-emerald-50 px-2.5 py-1.5 text-emerald-700">
                {savedMsg}
              </div>
            )}
          </div>
        )}

        <footer className="flex items-center justify-between border-t px-4 py-3">
          <div className="text-xs text-muted-foreground">
            {pendingChange ? 'alterações pendentes — salve e reinicie o servidor' : 'sem alterações'}
          </div>
          <Button
            size="sm"
            className="gap-1.5"
            disabled={saving || !pendingChange || (mode === 'custom' && !customHost.trim())}
            onClick={save}
          >
            <Save className="h-3.5 w-3.5" /> {saving ? 'salvando…' : 'salvar'}
          </Button>
        </footer>
      </div>
    </div>
  );
}

function UrlRow({
  url,
  label,
  copied,
  onCopy,
}: {
  url: string;
  label?: string;
  copied: string | null;
  onCopy: (u: string) => void;
}) {
  const isCopied = copied === url;
  return (
    <div className="flex items-center justify-between gap-2 rounded bg-white px-2 py-1">
      <code className="truncate text-xs">{url}</code>
      <div className="flex items-center gap-2">
        {label && <span className="text-xs text-muted-foreground">{label}</span>}
        <button
          type="button"
          onClick={() => onCopy(url)}
          className="text-slate-500 hover:text-slate-900"
          title="copiar"
        >
          {isCopied ? <Check className="h-3.5 w-3.5 text-emerald-600" /> : <Copy className="h-3.5 w-3.5" />}
        </button>
      </div>
    </div>
  );
}

function BindOption({
  active,
  onSelect,
  title,
  desc,
  warning,
}: {
  active: boolean;
  onSelect: () => void;
  title: string;
  desc: string;
  warning?: boolean;
}) {
  return (
    <label
      className={`flex cursor-pointer items-start gap-2 rounded-md border px-3 py-2 transition ${
        active
          ? warning
            ? 'border-amber-400 bg-amber-50'
            : 'border-indigo-500 bg-indigo-50'
          : 'border-slate-200 bg-white hover:bg-slate-50'
      }`}
    >
      <input
        type="radio"
        checked={active}
        onChange={onSelect}
        className="mt-0.5"
      />
      <div>
        <div className="font-medium text-slate-800">{title}</div>
        <div className="text-xs text-muted-foreground">{desc}</div>
      </div>
    </label>
  );
}
