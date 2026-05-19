import { useEffect, useState } from 'react';
import { KeyRound } from 'lucide-react';
import {
  Alert,
  Box,
  Button,
  Group,
  Modal,
  Stack,
  Text,
  Title,
} from '@/ui';
import { getVaultStatus } from '@/lib/vault';

interface Props {
  /** Chamado quando o usuário clica em "Criar cofre" — abre o VaultPanel. */
  onOpenVault: () => void;
}

/**
 * Detecta se o cofre não está inicializado ao montar a UI e mostra um
 * modal explicando que ele é necessário pra recursos como identidade
 * de agentes (keypair). Não bloqueia o app — o usuário pode dispensar
 * por essa sessão se quiser.
 *
 * O check roda uma vez (mount). Re-aparece em refresh enquanto vault
 * estiver sem init. Quando o user cria via VaultPanel, o estado é
 * `initialized=true` e o modal não aparece mais.
 */
export function VaultBootstrap({ onOpenVault }: Props) {
  const [needed, setNeeded] = useState(false);
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const s = await getVaultStatus();
        if (alive && !s.initialized) setNeeded(true);
      } catch {
        // Servidor inacessível — não força nada. UI principal já vai
        // dar feedback de "sem conexão" no fluxo normal.
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  if (!needed || dismissed) return null;

  return (
    <Modal
      opened
      onClose={() => setDismissed(true)}
      size="md"
      title={
        <Group gap="xs">
          <KeyRound size={18} />
          <Title order={5}>Cofre não criado</Title>
        </Group>
      }
    >
      <Stack gap="md">
        <Text>
          Esse projeto ainda não tem cofre. O cofre guarda secrets (chaves de
          API, tokens) e identidades de agentes (chave privada Ed25519). Sem
          ele, agentes não conseguem assinar handoffs e secrets de provider
          não ficam disponíveis.
        </Text>
        <Alert color="info">
          Você pode criar agora pela UI (recomendado) — ou pular por essa
          sessão. Se pular, recursos que dependem do cofre vão falhar com
          mensagem orientando a criar.
        </Alert>
        <Group justify="space-between">
          <Button variant="subtle" color="gray" onClick={() => setDismissed(true)}>
            pular por agora
          </Button>
          <Button
            onClick={() => {
              setDismissed(true);
              onOpenVault();
            }}
          >
            criar cofre agora
          </Button>
        </Group>
      </Stack>
    </Modal>
  );
}
