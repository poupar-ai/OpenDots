import type { ReactNode } from 'react';
import { Accordion as MAccordion } from '@mantine/core';

export type AccordionVariant = 'default' | 'contained' | 'filled' | 'separated';

export interface AccordionProps {
  value: string | null;
  onChange: (value: string | null) => void;
  variant?: AccordionVariant;
  children: ReactNode;
}

export function Accordion({
  value,
  onChange,
  variant = 'separated',
  children,
}: AccordionProps) {
  return (
    <MAccordion value={value} onChange={onChange} variant={variant}>
      {children}
    </MAccordion>
  );
}

export interface AccordionItemProps {
  value: string;
  children: ReactNode;
}

export function AccordionItem({ value, children }: AccordionItemProps) {
  return <MAccordion.Item value={value}>{children}</MAccordion.Item>;
}

export interface AccordionControlProps {
  children: ReactNode;
}

export function AccordionControl({ children }: AccordionControlProps) {
  return <MAccordion.Control>{children}</MAccordion.Control>;
}

export interface AccordionPanelProps {
  children: ReactNode;
}

export function AccordionPanel({ children }: AccordionPanelProps) {
  return <MAccordion.Panel>{children}</MAccordion.Panel>;
}
