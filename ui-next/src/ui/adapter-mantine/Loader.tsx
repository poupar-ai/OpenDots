import { Loader as MLoader } from '@mantine/core';
import { colorToMantine, sizeToMantine } from './tokens';
import type { Color, Size } from '../types';

export interface LoaderProps {
  size?: Size;
  color?: Color;
  type?: 'oval' | 'bars' | 'dots';
}

export function Loader({ size = 'sm', color = 'primary', type = 'oval' }: LoaderProps) {
  return <MLoader size={sizeToMantine[size]} color={colorToMantine[color]} type={type} />;
}
