import { Burger as MBurger } from '@mantine/core';
import { colorToMantine, sizeToMantine } from './tokens';
import type { Breakpoint, Color, Size } from '../types';

export interface BurgerProps {
  opened: boolean;
  onClick: () => void;
  /** Aria-label obrigatório — é um botão ícone-only. */
  label: string;
  size?: Size;
  color?: Color;
  /** Mostra só a partir deste breakpoint. */
  visibleFrom?: Breakpoint;
  /** Esconde a partir deste breakpoint. */
  hiddenFrom?: Breakpoint;
}

export function Burger({
  opened,
  onClick,
  label,
  size = 'md',
  color,
  visibleFrom,
  hiddenFrom,
}: BurgerProps) {
  return (
    <MBurger
      opened={opened}
      onClick={onClick}
      aria-label={label}
      size={sizeToMantine[size]}
      color={color ? colorToMantine[color] : undefined}
      visibleFrom={visibleFrom}
      hiddenFrom={hiddenFrom}
    />
  );
}
