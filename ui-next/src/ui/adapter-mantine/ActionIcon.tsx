import type { ReactNode } from 'react';
import { ActionIcon as MActionIcon } from '@mantine/core';
import { colorToMantine, sizeToMantine } from './tokens';
import type { Color, Size, Variant } from '../types';

export interface ActionIconProps {
  children: ReactNode;
  onClick?: () => void;
  disabled?: boolean;
  size?: Size;
  variant?: Variant;
  color?: Color;
  /** Aria-label obrigatório se for ícone-only. */
  label?: string;
}

export function ActionIcon({
  size = 'sm',
  variant = 'subtle',
  color = 'gray',
  label,
  ...rest
}: ActionIconProps) {
  return (
    <MActionIcon
      {...rest}
      size={sizeToMantine[size]}
      variant={variant}
      color={colorToMantine[color]}
      aria-label={label}
    />
  );
}
