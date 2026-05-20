import type { ReactNode } from 'react';
import { NavLink as MNavLink } from '@mantine/core';
import { colorToMantine } from './tokens';
import type { Color } from '../types';

export interface NavLinkProps {
  label: ReactNode;
  /** Subtítulo opcional exibido abaixo do label. */
  description?: ReactNode;
  leftIcon?: ReactNode;
  rightSection?: ReactNode;
  onClick?: () => void;
  active?: boolean;
  color?: Color;
}

export function NavLink({
  label,
  description,
  leftIcon,
  rightSection,
  onClick,
  active,
  color,
}: NavLinkProps) {
  return (
    <MNavLink
      label={label}
      description={description}
      leftSection={leftIcon}
      rightSection={rightSection}
      onClick={onClick}
      active={active}
      color={color ? colorToMantine[color] : undefined}
    />
  );
}
