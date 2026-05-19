import type { ReactNode } from 'react';
import { Switch as MSwitch } from '@mantine/core';
import { colorToMantine, sizeToMantine } from './tokens';
import type { Color, Size } from '../types';

export interface SwitchProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label?: ReactNode;
  disabled?: boolean;
  size?: Size;
  color?: Color;
}

export function Switch({
  checked,
  onChange,
  label,
  disabled,
  size = 'sm',
  color = 'primary',
}: SwitchProps) {
  return (
    <MSwitch
      checked={checked}
      onChange={(e) => onChange(e.currentTarget.checked)}
      label={label}
      disabled={disabled}
      size={sizeToMantine[size]}
      color={colorToMantine[color]}
    />
  );
}
