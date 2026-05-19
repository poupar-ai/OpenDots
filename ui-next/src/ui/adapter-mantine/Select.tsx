import { Select as MSelect } from '@mantine/core';
import { sizeToMantine } from './tokens';
import type { Size } from '../types';

export interface SelectOption {
  value: string;
  label: string;
}

export interface SelectProps {
  value: string | null;
  onChange: (v: string | null) => void;
  data: SelectOption[] | string[];
  label?: string;
  placeholder?: string;
  disabled?: boolean;
  size?: Size;
  /** Permite digitar valor não-listado (UI dispara `onChange` com a string). */
  searchable?: boolean;
  error?: string | null;
}

export function Select({
  value,
  onChange,
  data,
  label,
  placeholder,
  disabled,
  size = 'sm',
  searchable,
  error,
}: SelectProps) {
  return (
    <MSelect
      value={value}
      onChange={onChange}
      data={data as never}
      label={label}
      placeholder={placeholder}
      disabled={disabled}
      size={sizeToMantine[size]}
      searchable={searchable}
      error={error || undefined}
    />
  );
}
