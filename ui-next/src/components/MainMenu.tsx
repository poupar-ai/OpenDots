import { useState, type ReactNode } from 'react';
import { Box, Burger, Button, Drawer, Group, NavLink, Stack, colorVar } from '@/ui';
import type { Breakpoint, Color } from '@/ui';

/**
 * Um item do menu principal. Data-driven: adicionar uma entrada ao array
 * passado em `items` basta pra surgir tanto no desktop (botão inline)
 * quanto no mobile (linha do Drawer). Nenhum layout a mais pra escrever.
 */
export interface MenuItem {
  id: string;
  /** Texto do botão. */
  label: string;
  /** Tooltip nativo (desktop). */
  title?: string;
  icon: ReactNode;
  /** Tint semântico do ícone (ex.: 'danger' p/ proteções). Default: herda. */
  iconColor?: Color;
  onClick: () => void;
}

export interface MainMenuProps {
  items: MenuItem[];
  /** A partir deste breakpoint mostra inline; abaixo, colapsa no Drawer. */
  breakpoint?: Breakpoint;
}

/** Tinge o ícone via currentColor sem amarrar o item ao vendor. */
function tinted(icon: ReactNode, color?: Color): ReactNode {
  return (
    <span style={{ display: 'inline-flex', color: color ? colorVar(color, 6) : undefined }}>
      {icon}
    </span>
  );
}

/**
 * Menu principal responsivo (padrão Mantine: Burger + Drawer).
 * - `>= breakpoint`: botões inline (Group).
 * - `< breakpoint`: hambúrguer que abre um Drawer lateral com as ações
 *   empilhadas (Stack).
 */
export function MainMenu({ items, breakpoint = 'lg' }: MainMenuProps) {
  const [opened, setOpened] = useState(false);

  return (
    <>
      {/* Desktop — botões inline, visual de toolbar neutro. */}
      <Box visibleFrom={breakpoint}>
        <Group gap="xs" wrap="wrap">
          {items.map((it) => (
            <Button
              key={it.id}
              variant="default"
              color="gray"
              title={it.title}
              leftIcon={tinted(it.icon, it.iconColor)}
              onClick={it.onClick}
            >
              {it.label}
            </Button>
          ))}
        </Group>
      </Box>

      {/* Mobile — hambúrguer + drawer com navegação alinhada à esquerda. */}
      <Burger
        hiddenFrom={breakpoint}
        opened={opened}
        onClick={() => setOpened((o) => !o)}
        label={opened ? 'fechar menu' : 'abrir menu'}
      />
      <Drawer
        opened={opened}
        onClose={() => setOpened(false)}
        title="Menu"
        position="right"
        size="xs"
      >
        <Stack gap={2}>
          {items.map((it) => (
            <NavLink
              key={it.id}
              label={it.label}
              description={it.title}
              leftIcon={tinted(it.icon, it.iconColor)}
              onClick={() => {
                setOpened(false);
                it.onClick();
              }}
            />
          ))}
        </Stack>
      </Drawer>
    </>
  );
}
