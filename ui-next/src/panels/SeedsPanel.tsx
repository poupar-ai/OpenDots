import { useEffect, useMemo, useState } from 'react';
import { Package, Check, AlertCircle, CheckSquare, Square } from 'lucide-react';
import {
  Accordion,
  AccordionControl,
  AccordionItem,
  AccordionPanel,
  Alert,
  Badge,
  Box,
  Button,
  Chip,
  Divider,
  Group,
  Loader,
  Modal,
  Stack,
  Switch,
  Text,
  Title,
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
  // Conflict mode: "skip" (default), "overwrite" ou "add_new". Os 2 toggles
  // são mutuamente exclusivos — ligar um desliga o outro.
  const [conflictMode, setConflictMode] = useState<'skip' | 'overwrite' | 'add_new'>('skip');
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

  function selectAll(ref: string, allAgents: string[]) {
    setPicked((prev) => ({ ...prev, [ref]: new Set(allAgents) }));
  }

  function clearAll(ref: string) {
    setPicked((prev) => ({ ...prev, [ref]: new Set() }));
  }

  async function install(ref: string, allAgents: string[]) {
    setApplying(ref);
    setApplyErr(null);
    setResult(null);
    try {
      const sel = getPicked(ref, allAgents);
      const only = sel.size === allAgents.length ? undefined : Array.from(sel);
      const r = await applySeed({
        ref,
        only,
        overwrite: conflictMode === 'overwrite',
        addNew: conflictMode === 'add_new',
      });
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
        <Group gap="sm" align="center">
          <Package size={20} />
          <Box>
            <Title order={4}>Instalar time de agentes</Title>
            <Text size="sm" muted>
              projeto: {projectRoot || '…'}
            </Text>
          </Box>
        </Group>
      }
    >
      <Stack gap="lg">
        {loadErr && <Alert color="danger" icon={<AlertCircle size={16} />}>{loadErr}</Alert>}

        {!categories && !loadErr && (
          <Group gap="sm">
            <Loader size="sm" />
            <Text size="sm" muted>Carregando packs disponíveis…</Text>
          </Group>
        )}

        {categories && categories.length === 0 && (
          <Text size="sm" muted>Nenhum pack disponível.</Text>
        )}

        {categories?.map((cat) => (
          <Stack key={cat.name} gap="sm">
            <Box>
              <Title order={5}>{cat.label}</Title>
              {cat.description && (
                <Text size="sm" muted>{cat.description}</Text>
              )}
            </Box>

            <Accordion
              value={expandedPack}
              onChange={setExpandedPack}
              variant="separated"
            >
              {cat.packs.map((p) => {
                const ref = `${cat.name}/${p.name}`;
                const sel = getPicked(ref, p.agents);
                const allSelected = sel.size === p.agents.length;
                const noneSelected = sel.size === 0;

                return (
                  <AccordionItem key={ref} value={ref}>
                    <AccordionControl>
                      <Group justify="space-between" gap="sm" wrap="nowrap">
                        <Group gap="sm">
                          <Text size="md" fw={600}>{p.label}</Text>
                          <Badge variant="light" color="primary">
                            {p.agents.length} agentes
                          </Badge>
                        </Group>
                        <Text size="xs" muted style={{ fontFamily: 'monospace' }}>
                          {ref}
                        </Text>
                      </Group>
                    </AccordionControl>

                    <AccordionPanel>
                      <Stack gap="md">
                        {p.description && (
                          <Text size="sm">{p.description}</Text>
                        )}

                        <Box>
                          <Group justify="space-between" gap="sm" align="center">
                            <Text size="sm" fw={600}>
                              {sel.size} de {p.agents.length} selecionados
                            </Text>
                            <Group gap="xs">
                              <Button
                                variant="subtle"
                                size="xs"
                                color="primary"
                                disabled={allSelected}
                                leftIcon={<CheckSquare size={14} />}
                                onClick={() => selectAll(ref, p.agents)}
                              >
                                Selecionar todos
                              </Button>
                              <Button
                                variant="subtle"
                                size="xs"
                                color="gray"
                                disabled={noneSelected}
                                leftIcon={<Square size={14} />}
                                onClick={() => clearAll(ref)}
                              >
                                Limpar
                              </Button>
                            </Group>
                          </Group>
                        </Box>

                        <Group gap="xs" wrap="wrap">
                          {p.agents.map((a) => (
                            <Chip
                              key={a}
                              checked={sel.has(a)}
                              onChange={() => toggleAgent(ref, a, p.agents)}
                              size="sm"
                              variant="outline"
                            >
                              {a}
                            </Chip>
                          ))}
                        </Group>

                        <Divider />

                        <Group justify="space-between" gap="md" align="center" wrap="wrap">
                          <Group gap="md" align="center">
                            <Switch
                              checked={conflictMode === 'overwrite'}
                              onChange={(on) =>
                                setConflictMode(on ? 'overwrite' : 'skip')
                              }
                              label="Sobrescrever existentes"
                              size="sm"
                            />
                            <Switch
                              checked={conflictMode === 'add_new'}
                              onChange={(on) =>
                                setConflictMode(on ? 'add_new' : 'skip')
                              }
                              label="Add as new (suffix -2, -3…)"
                              size="sm"
                              color="success"
                            />
                          </Group>
                          <Button
                            size="sm"
                            color="primary"
                            disabled={sel.size === 0 || applying === ref}
                            loading={applying === ref}
                            leftIcon={<Check size={16} />}
                            onClick={() => install(ref, p.agents)}
                          >
                            Instalar {sel.size} {sel.size === 1 ? 'agente' : 'agentes'}
                          </Button>
                        </Group>
                      </Stack>
                    </AccordionPanel>
                  </AccordionItem>
                );
              })}
            </Accordion>
          </Stack>
        ))}

        {applyErr && (
          <Alert color="danger" icon={<AlertCircle size={16} />}>
            {applyErr}
          </Alert>
        )}

        {result && (
          <Alert
            color="success"
            icon={<Check size={16} />}
            title={`${result.ref} instalado — ${summary}`}
          >
            <Stack gap={6}>
              {result.r.added.length > 0 && (
                <Text size="sm">+ {result.r.added.join(', ')}</Text>
              )}
              {result.r.replaced.length > 0 && (
                <Text size="sm">↻ {result.r.replaced.join(', ')}</Text>
              )}
              {result.r.skipped.length > 0 && (
                <Text size="sm" muted>
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
