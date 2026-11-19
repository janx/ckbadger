'use client';

import { useRef, type KeyboardEvent } from 'react';
import { chipClass } from '@/components/ui/filter-chips';
import { rovingTarget } from '@/lib/roving-focus';
import { cn } from '@/lib/utils';

/** One span of a range: its key and its words (`1 w`, `30 d`, `24 h`, `all`). */
export interface RangeOption<K extends string> {
  key: K;
  label: string;
}

interface RangeSelectProps<K extends string> {
  /** The range's name for a screen reader. */
  label: string;
  options: readonly RangeOption<K>[];
  value: K;
  onChange: (key: K) => void;
  className?: string;
}

/**
 * The one spelling of a span (the controls board, gate D0′): a whole number, one space, a lowercase
 * unit — `h`, `d`, `w`, `m` (a month), `y` — or `all`. `1 w · 1 m · 1 y · all`, `30 d · 90 d · 1 y ·
 * all`, `24 h · 14 d`.
 */
const SPAN = /^(?:[1-9]\d* [hdwmy]|all)$/;

/**
 * A range (the controls board, gate D0′; UI review 2026-11-14, phase C): the filter chips joined —
 * each after the first lies over the line of the one before it, the chosen one on top so its
 * celadon line is whole. One span at a time: a radio group, as the filter chips are, with the same
 * keyboard. A label that is not the one spelling of a span is refused.
 */
export function RangeSelect<K extends string>({
  label,
  options,
  value,
  onChange,
  className,
}: RangeSelectProps<K>) {
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  for (const option of options)
    if (!SPAN.test(option.label))
      throw new Error(
        `RangeSelect "${label}": "${option.label}" is not a span: a number, a space and a lowercase unit (h, d, w, m, y), or all`
      );
  const index = options.findIndex((option) => option.key === value);
  if (index < 0) throw new Error(`RangeSelect "${label}": no span ${value}`);

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const to = rovingTarget(event.key, index, options.length, 'any');
    if (to === null) return;
    event.preventDefault();
    onChange(options[to].key);
    refs.current[to]?.focus();
  };

  return (
    <div
      role="radiogroup"
      aria-label={label}
      onKeyDown={onKeyDown}
      className={cn('inline-flex', className)}
    >
      {options.map((option, i) => {
        const checked = i === index;
        return (
          <button
            key={option.key}
            ref={(el) => {
              refs.current[i] = el;
            }}
            type="button"
            role="radio"
            aria-checked={checked}
            tabIndex={checked ? 0 : -1}
            onClick={() => onChange(option.key)}
            className={cn(
              chipClass(checked, false),
              i > 0 && '-ml-px',
              checked && 'relative z-[1]'
            )}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}
