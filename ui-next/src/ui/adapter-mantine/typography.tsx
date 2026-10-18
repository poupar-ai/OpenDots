import type { ReactNode, CSSProperties } from 'react';
import { Text as MText, Title as MTitle, Code as MCode, Badge as MBadge, Divider as MDivider } from '@mantine/core';
import { colorToMantine, sizeToMantine } from './tokens';
import type { Color, Size } from '../types';

// ── Text ────────────────────────────────────────────────────────────────────

export interface TextProps {
  children: ReactNode;
  size?: Size;
  fw?: 400 | 500 | 600 | 700;
  /** "dimmed" pra texto secundário. */
  muted?: boolean;
  color?: Color;
  style?: CSSProperties;
  component?: 'span' | 'p' | 'div';
}

export function Text({ size = 'sm', fw, muted, color, style, component, children }: TextProps) {
  return (
    <MText
      size={sizeToMantine[size]}
      fw={fw}
      c={muted ? 'dimmed' : color ? colorToMantine[color] : undefined}
      style={style}
      component={component}
    >
      {children}
    </MText>
  );
}

// ── Title ───────────────────────────────────────────────────────────────────

export interface TitleProps {
  children: ReactNode;
  order?: 1 | 2 | 3 | 4 | 5 | 6;
}

export function Title({ order = 4, children }: TitleProps) {
  return <MTitle order={order}>{children}</MTitle>;
}

// ── Code ────────────────────────────────────────────────────────────────────

export interface CodeProps {
  children: ReactNode;
  style?: CSSProperties;
  /** Bloco (`<pre>`) em vez de inline. */
  block?: boolean;
}

export function Code({ children, style, block }: CodeProps) {
  return <MCode style={style} block={block}>{children}</MCode>;
}

// ── Badge ───────────────────────────────────────────────────────────────────

export interface BadgeProps {
  children: ReactNode;
  color?: Color;
  variant?: 'light' | 'filled' | 'outline';
  size?: Size;
}

export function Badge({ color = 'gray', variant = 'light', size = 'sm', children }: BadgeProps) {
  return (
    <MBadge color={colorToMantine[color]} variant={variant} size={sizeToMantine[size]}>
      {children}
    </MBadge>
  );
}

// ── Divider ─────────────────────────────────────────────────────────────────

export function Divider() {
  return <MDivider />;
}
