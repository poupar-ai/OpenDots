import { useEffect, useMemo, useState } from 'react';
import { Package, Check, AlertCircle, ChevronRight, ChevronDown } from 'lucide-react';
import {
  Alert,
  Badge,
  Box,
  Button,
  Checkbox,
  Chip,
  Group,
  Modal,
  Paper,
  Stack,
  Text,
  Title,
  UnstyledButton,
} from '@/ui';
import {
  listSeeds,
  applySeed,
  type SeedCategory,
  type SeedApplyResult,
} from '@/lib/seeds';

interface Props {
  onClose: () => void;
  onInstalled?: () => void;
}

export function SeedsPanel({ onClose, onInstalled }: Props) {
  const [categories, setCategories] = useState<SeedCategory[] | null>(null);
  const [projectRoot, setProjectRoot] = useState<string>('');
  const [loadErr, setLoadErr] = useState<string | null>(null);

  const [expandedPack, setExpandedPack] = useState<string | null>(null);
  const [picked, setPicked] = useState<Record<string, Set<string>>>({});
  const [overwrite, setOverwrite] = useState(false);
  const [applying, setApplying] = useState<string | null>(null);
  const [result, setResult] = useState<{ ref: string; r: SeedApplyResult } | null>(null);
  const [applyErr, setApplyErr] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const j = await listSeeds();
        if (!alive) return;
        setCategories(j.categories);
        setProjectRoot(j.project_root);
        if (j.categories.length === 1 && j.categories[0].packs.length === 1) {
          setExpandedPack(`${j.categories[0].name}/${j.categories[0].packs[0].name}`);
        }
      } catch (e: any) {
        if (alive) setLoadErr(e?.message || 'falha ao carregar');
      }
    })();
    return () => { alive = false; };
  }, []);

  function toggleAgent(ref: string, agent: string, allAgents: string[]) {
    setPicked((prev) => {
      const cur = new Set(prev[ref] ?? allAgents);
      if (cur.has(agent)) cur.delete(agent);
      else cur.add(agent);
      return { ...prev, [ref]: cur };
    });
  }

  function getPicked(ref: string, allAgents: string[]): Set<string> {
    return picked[ref] ?? new Set(allAgents);
  }

  function resetPicked(ref: string, allAgents: string[]) {
    setPicked((prev) => ({ ...prev, [ref]: new Set(allAgents) }));
  }

  async function install(ref: string, allAgents: string[]) {
    setApplying(ref);
    setApplyErr(null);
    setResult(null);
    try {
      const sel = getPicked(ref, allAgents);
      const only = sel.size === allAgents.length ? undefined : Array.from(sel);
      const r = await applySeed({ ref, only, overwrite });
      setResult({ ref, r });
      onInstalled?.();
    } catch (e: any) {
      setApplyErr(e?.message || 'falha ao instalar');
    } finally {
      setApplying(null);
    }
  }

  const summary = useMemo(() => {
    if (!result) return null;
    const { r } = result;
    const parts: string[] = [];
    if (r.added.length)    parts.push(`${r.added.length} adicionados`);
    if (r.replaced.length) parts.push(`${r.replaced.length} substituídos`);
    if (r.skipped.length)  parts.push(`${r.skipped.length} preservados`);
    return parts.join(' · ');
  }, [result]);

  return (
    <Modal
      opened
      onClose={onClose}
      size="xl"
      title={
        <Group gap="xs">
          <Package size={18} />
          <Box>
            <Title order={5}>Instalar time de agentes</Title>
            <Text size="xs" muted>
              projeto: {projectRoot || '…'}
            </Text>
          </Box>
        </Group>
      }
    >
      <Stack gap="md">
        {loadErr && <Alert color="danger">{loadErr}</Alert>}

        {!categories && !loadErr && (
          <Text size="xs" muted>carregando…</Text>
        )}

        {categories && categories.length === 0 && (
          <Text size="xs" muted>Nenhum pack disponível.</Text>
        )}

        {categories?.map((cat) => (
          <Stack key={cat.name} gap="xs">
            <Title order={6}>{cat.label}</Title>
            {cat.description && (
              <Text size="xs" muted>{cat.description}</Text>
            )}

            <Stack gap="xs">
              {cat.packs.map((p) => {
                const ref = `${cat.name}/${p.name}`;
                const expanded = expandedPack === ref;
                const sel = getPicked(ref, p.agents);
                const all = sel.size === p.agents.length;
                return (
                  <Paper key={ref} withBorder padding="sm">
                    <Stack gap="sm">
                      <UnstyledButton
                        fullWidth
                        onClick={() => setExpandedPack(expanded ? null : ref)}
                      >
                        <Group justify="space-between" gap="xs">
                          <Group gap="xs">
                            {expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
                            <Text size="sm" fw={600}>{p.label}</Text>
                            <Badge variant="outline">{p.agents.length} agentes</Badge>
                          </Group>
                          <Text size="xs" muted style={{ fontFamily: 'monospace' }}>
                            {ref}
                          </Text>
                        </Group>
                      </UnstyledButton>

                      {expanded && (
                        <Stack gap="sm">
                          {p.description && (
                            <Text size="xs" muted>{p.description}</Text>
                          )}

                          <Group justify="space-between" gap="xs">
                            <Text size="xs" fw={600} muted>
                              Selecionados: {sel.size}/{p.agents.length}
                            </Text>
                            <Button
                              variant="subtle"
                              size="xs"
                              color="gray"
                              onClick={() => resetPicked(ref, p.agents)}
                            >
                              todos
                            </Button>
                          </Group>

                          <Group gap={6} wrap="wrap">
                            {p.agents.map((a) => (
                              <Chip
                                key={a}
                                checked={sel.has(a)}
                                onChange={() => toggleAgent(ref, a, p.agents)}
                                size="xs"
                                variant="outline"
                              >
                                {a}
                              </Chip>
                            ))}
                          </Group>

                          <Group justify="space-between" gap="xs">
                            <Checkbox
                              checked={overwrite}
                              onChange={setOverwrite}
                              label="sobrescrever existentes"
                              size="xs"
                            />
                            <Button
                              size="xs"
                              disabled={sel.size === 0 || applying === ref}
                              loading={applying === ref}
                              leftIcon={<Check size={14} />}
                              onClick={() => install(ref, p.agents)}
                            >
                              Instalar {all ? 'todos' : `${sel.size}`}
                            </Button>
                          </Group>
                        </Stack>
                      )}
                    </Stack>
                  </Paper>
                );
              })}
            </Stack>
          </Stack>
        ))}

        {applyErr && (
          <Alert color="danger" icon={<AlertCircle size={14} />}>
            {applyErr}
          </Alert>
        )}

        {result && (
          <Alert
            color="success"
            icon={<Check size={14} />}
            title={`${result.ref} instalado — ${summary}`}
          >
            <Stack gap={4}>
              {result.r.added.length > 0 && (
                <Text size="xs">+ {result.r.added.join(', ')}</Text>
              )}
              {result.r.replaced.length > 0 && (
                <Text size="xs">↻ {result.r.replaced.join(', ')}</Text>
              )}
              {result.r.skipped.length > 0 && (
                <Text size="xs" muted>
                  = preservados (use "sobrescrever"): {result.r.skipped.join(', ')}
                </Text>
              )}
            </Stack>
          </Alert>
        )}
      </Stack>
    </Modal>
  );
}
