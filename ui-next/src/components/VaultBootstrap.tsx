import { useEffect, useState } from 'react';
import { KeyRound } from 'lucide-react';
import { Button, Group, Modal, Stack, Text, TextInput, Title } from '@/ui';
import { getVaultStatus, initVault, unlockVault } from '@/lib/vault';

interface Props {
  /** Avisa o App quando o cofre está pronto (inicializado + destrancado). */
  onReady: (ready: boolean) => void;
}

type Phase = 'checking' | 'create' | 'unlock' | 'ready';

/**
 * Gate OBRIGATÓRIO do cofre — bloqueia o app até o cofre estar inicializado e
 * destrancado, porque a senha dos humanos é guardada nele. Sem isso, criar
 * perfil falha (e deixa órfão). Não é dispensável: cria ou destranca inline.
 */
export function VaultBootstrap({ onReady }: Props) {
  const [phase, setPhase] = useState<Phase>('checking');
  const [pw, setPw] = useState('');
  const [pw2, setPw2] = useState('');
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const s = await getVaultStatus();
        if (!alive) return;
        if (!s.initialized) setPhase('create');
        else if (s.locked) setPhase('unlock');
        else { setPhase('ready'); onReady(true); }
      } catch {
        // Servidor inacessível — não trava o app (o fluxo normal já dá
        // feedback de offline). Libera e deixa seguir.
        if (alive) { setPhase('ready'); onReady(true); }
      }
    })();
    return () => { alive = false; };
  }, [onReady]);

  if (phase === 'checking' || phase === 'ready') return null;

  const creating = phase === 'create';

  const submit = async () => {
    setErr(null);
    if (pw.length < 4) { setErr('mín. 4 caracteres'); return; }
    if (creating && pw !== pw2) { setErr('as senhas não conferem'); return; }
    setBusy(true);
    try {
      if (creating) await initVault(pw);
      else await unlockVault(pw);
      setPhase('ready');
      onReady(true);
    } catch (e: any) {
      setErr(e?.message || 'falhou');
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      opened
      onClose={() => {}}
      withCloseButton={false}
      size="sm"
      title={
        <Group gap="xs">
          <KeyRound size={18} />
          <Title order={5}>{creating ? 'Criar o cofre' : 'Destrancar o cofre'}</Title>
        </Group>
      }
    >
      <form onSubmit={(e) => { e.preventDefault(); void submit(); }}>
        <Stack gap="md">
          <Text size="sm" muted>
            {creating
              ? 'O cofre guarda secrets e a senha dos humanos (só o hash). Defina uma senha-mestra — ela é obrigatória antes de criar qualquer perfil.'
              : 'O cofre está trancado. Informe a senha-mestra pra destrancar e poder entrar/criar perfis.'}
          </Text>
          <TextInput
            type="password"
            value={pw}
            onChange={setPw}
            placeholder={creating ? 'senha-mestra do cofre' : 'senha-mestra'}
          />
          {creating && (
            <TextInput type="password" value={pw2} onChange={setPw2} placeholder="confirme a senha" />
          )}
          {err && <Text size="xs" color="danger">{err}</Text>}
          <Group justify="flex-end">
            <Button type="submit" loading={busy} disabled={busy || pw.length < 4}>
              {creating ? 'Criar cofre' : 'Destrancar'}
            </Button>
          </Group>
        </Stack>
      </form>
    </Modal>
  );
}
