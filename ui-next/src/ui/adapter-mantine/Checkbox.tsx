import type { ReactNode } from 'react';
import { Checkbox as MCheckbox } from '@mantine/core';
import { colorToMantine, sizeToMantine } from './tokens';
import type { Color, Size } from '../types';

export interface CheckboxProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label?: ReactNode;
  disabled?: boolean;
  size?: Size;
  color?: Color;
}

export function Checkbox({
  checked,
  onChange,
  label,
  disabled,
  size = 'sm',
  color = 'primary',
}: CheckboxProps) {
  return (
    <MCheckbox
      checked={checked}
      onChange={(e) => onChange(e.currentTarget.checked)}
      label={label}
      disabled={disabled}
      size={sizeToMantine[size]}
      color={colorToMantine[color]}
    />
  );
}
