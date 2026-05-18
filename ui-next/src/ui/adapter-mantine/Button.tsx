import type { ReactNode } from 'react';
import { Button as MButton } from '@mantine/core';
import { colorToMantine, sizeToMantine } from './tokens';
import type { Color, Size, Variant } from '../types';

export interface ButtonProps {
  children: ReactNode;
  onClick?: () => void;
  disabled?: boolean;
  loading?: boolean;
  size?: Size | 'compact-xs' | 'compact-sm';
  variant?: Variant;
  color?: Color;
  leftIcon?: ReactNode;
  rightIcon?: ReactNode;
  fullWidth?: boolean;
  type?: 'button' | 'submit' | 'reset';
}

export function Button({
  size = 'sm',
  variant = 'filled',
  color = 'primary',
  leftIcon,
  rightIcon,
  fullWidth,
  type,
  ...rest
}: ButtonProps) {
  const mSize =
    size === 'compact-xs' || size === 'compact-sm'
      ? size
      : sizeToMantine[size];
  return (
    <MButton
      {...rest}
      size={mSize}
      variant={variant}
      color={colorToMantine[color]}
      leftSection={leftIcon}
      rightSection={rightIcon}
      fullWidth={fullWidth}
      type={type}
    />
  );
}
