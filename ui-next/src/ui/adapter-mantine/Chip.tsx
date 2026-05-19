import type { ReactNode } from 'react';
import { Chip as MChip } from '@mantine/core';
import { colorToMantine, sizeToMantine } from './tokens';
import type { Color, Size } from '../types';

export interface ChipProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  children: ReactNode;
  variant?: 'outline' | 'filled' | 'light';
  color?: Color;
  size?: Size;
  disabled?: boolean;
}

export function Chip({
  checked,
  onChange,
  children,
  variant = 'outline',
  color = 'primary',
  size = 'sm',
  disabled,
}: ChipProps) {
  return (
    <MChip
      checked={checked}
      onChange={onChange}
      variant={variant}
      color={colorToMantine[color]}
      size={sizeToMantine[size]}
      disabled={disabled}
    >
      {children}
    </MChip>
  );
}
