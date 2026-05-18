// Maps de tokens semânticos → tokens do Mantine.
//
// Único arquivo no adapter que conhece os nomes do vendor (paleta,
// tamanhos). Quem trocar de lib reescreve só esse arquivo + os wrappers.

import type { Color, Size, Variant, Radius, Spacing } from '../types';

export const colorToMantine: Record<Color, string> = {
  primary: 'indigo',
  success: 'teal',
  warning: 'yellow',
  danger:  'red',
  info:    'sky',
  gray:    'gray',
};

export const sizeToMantine: Record<Size, 'xs' | 'sm' | 'md' | 'lg'> = {
  xs: 'xs',
  sm: 'sm',
  md: 'md',
  lg: 'lg',
};

export const radiusToMantine: Record<Radius, string> = {
  none: '0',
  sm:   'sm',
  md:   'md',
  lg:   'lg',
  full: 'xl',
};

export const spacingToMantine: Record<Spacing, 'xs' | 'sm' | 'md' | 'lg' | 'xl'> = {
  xs: 'xs',
  sm: 'sm',
  md: 'md',
  lg: 'lg',
  xl: 'xl',
};

// Mantine não tem todos os variants nomeados igualmente em todos os
// componentes (Button: filled/outline/subtle/default/light; Alert:
// filled/outline/light). O wrapper de cada componente faz o mapping
// específico se necessário.
export const variantToMantine: Record<Variant, string> = {
  filled:  'filled',
  outline: 'outline',
  subtle:  'subtle',
  default: 'default',
  light:   'light',
};
