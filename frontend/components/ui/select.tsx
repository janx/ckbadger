'use client';

import { useId } from 'react';
import { FIELD_WELL } from '@/components/ui/text-field';
import { cn } from '@/lib/utils';

/** One choice of a select: the value it answers and the words it shows. */
export interface SelectOption<V extends string> {
  value: V;
  label: string;
}

interface SelectProps<V extends string> {
  /** The label it stands after (`sorted by`, `standard`), in the quiet ink. */
  label: string;
  value: V;
  options: readonly SelectOption<V>[];
  onChange: (value: V) => void;
  className?: string;
}

/**
 * A select (the controls board, gate D0′; UI review 2026-11-14, phase C): a real `<select>` in
 * the field's well (components/ui/text-field.tsx `FIELD_WELL`), 13 px, its chosen value in the
 * reading ink, the board's chevron after it in the quiet ink; it always stands after its label,
 * which is quiet and names it (`<label for>`). The list it opens is the browser's own.
 */
export function Select<V extends string>({
  label,
  value,
  options,
  onChange,
  className,
}: SelectProps<V>) {
  const id = useId();
  if (!options.some((option) => option.value === value))
    throw new Error(`Select "${label}": no option ${value}`);
  return (
    <span className={cn('inline-flex min-w-0 items-center gap-2.5', className)}>
      <label htmlFor={id} className="text-small text-text-dim whitespace-nowrap">
        {label}
      </label>
      <span className={cn(FIELD_WELL, 'relative px-0')}>
        <select
          id={id}
          value={value}
          onChange={(event) => onChange(event.target.value as V)}
          className="text-small text-text h-full min-w-0 appearance-none bg-transparent pl-2.5 pr-[25px] font-mono focus-visible:outline-none"
        >
          {options.map((option) => (
            <option key={option.value} value={option.value} className="bg-base-void text-text">
              {option.label}
            </option>
          ))}
        </select>
        <svg
          aria-hidden="true"
          width="9"
          height="6"
          viewBox="0 0 9 6"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.4"
          className="text-text-dim pointer-events-none absolute right-2.5"
        >
          <path d="M1 1l3.5 3.5L8 1" />
        </svg>
      </span>
    </span>
  );
}
