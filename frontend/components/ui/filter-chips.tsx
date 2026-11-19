'use client';

import { useRef, type KeyboardEvent } from 'react';
import { Select } from '@/components/ui/select';
import { useFromSm } from '@/hooks/useFromMd';
import { groupCount } from '@/lib/format-count';
import { color, type PaletteKey } from '@/lib/palette';
import { rovingTarget } from '@/lib/roving-focus';
import { cn } from '@/lib/utils';

/** One value of a filter: its key, its words, its count where it has one, its 7 px mark. */
export interface FilterValue<K extends string> {
  key: K;
  label: string;
  count?: number | null;
  /** The palette colour of the square before its words (a kind's, a class's). */
  mark?: PaletteKey;
}

interface FilterChipsProps<K extends string> {
  /** The filter's name: the radio group's for a screen reader, the select's label on a phone. */
  label: string;
  values: readonly FilterValue<K>[];
  value: K;
  onChange: (key: K) => void;
  className?: string;
}

/** The most values a phone draws as chips; more are a select after the filter's label. */
const PHONE_CHIPS = 4;

/**
 * A chip's box (the controls board, gate D0′): 28 px high from 640 and 32 below it, square, 10 px
 * in, the frame line; its words 13 px lowercase. The checked one is the celadon line and the
 * bright ink (one selected state on both grounds: no fill); another the reading ink, a zero's
 * words quiet (never faded). The range draws the same chips joined (components/ui/range-select.tsx).
 */
export function chipClass(checked: boolean, zero: boolean): string {
  return cn(
    'text-small inline-flex h-8 items-center gap-1.5 whitespace-nowrap border px-2.5 lowercase transition-colors sm:h-7',
    checked
      ? 'border-jade text-text-bright'
      : cn('border-base-border hover:text-jade', zero ? 'text-text-dim' : 'text-text')
  );
}

/**
 * A filter's values as chips (the controls board, gate D0′; UI review 2026-11-14, phase C): the
 * count 12 px in the quiet ink after its words, an optional 7 px mark before them. A filter holds
 * one value at a time ("all" or one kind), so it is a radio group, not toggle buttons: a pressed
 * toggle reads as a switch that a second press turns off, which none of the site's filters does;
 * "one of N" is what it is, the phone's select says the same, and the arrows move between the
 * values with one stop in the tab order. More than four values below 640 are drawn as that select,
 * after the filter's label.
 */
export function FilterChips<K extends string>({
  label,
  values,
  value,
  onChange,
  className,
}: FilterChipsProps<K>) {
  const fromSm = useFromSm();
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const index = values.findIndex((v) => v.key === value);
  if (index < 0) throw new Error(`FilterChips "${label}": no value ${value}`);
  const words = (v: FilterValue<K>) =>
    v.count === undefined || v.count === null ? v.label : `${v.label} ${groupCount(v.count)}`;

  if (!fromSm && values.length > PHONE_CHIPS)
    return (
      <Select
        label={label}
        value={value}
        options={values.map((v) => ({ value: v.key, label: words(v) }))}
        onChange={onChange}
        className={className}
      />
    );

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const to = rovingTarget(event.key, index, values.length, 'any');
    if (to === null) return;
    event.preventDefault();
    onChange(values[to].key);
    refs.current[to]?.focus();
  };

  return (
    <div
      role="radiogroup"
      aria-label={label}
      onKeyDown={onKeyDown}
      className={cn('flex flex-wrap gap-1.5', className)}
    >
      {values.map((v, i) => {
        const checked = i === index;
        return (
          <button
            key={v.key}
            ref={(el) => {
              refs.current[i] = el;
            }}
            type="button"
            role="radio"
            aria-checked={checked}
            tabIndex={checked ? 0 : -1}
            onClick={() => onChange(v.key)}
            className={chipClass(checked, v.count === 0)}
          >
            {v.mark && (
              <span
                aria-hidden="true"
                data-mark=""
                className="h-[7px] w-[7px] flex-none"
                style={{ backgroundColor: color(v.mark) }}
              />
            )}
            <span>{v.label}</span>
            {v.count !== undefined && v.count !== null && (
              <>
                <span className="sr-only"> </span>
                {/* a count: 12 px in the quiet ink, at every state of its chip */}
                <span className="text-meta text-text-dim">{groupCount(v.count)}</span>
              </>
            )}
          </button>
        );
      })}
    </div>
  );
}
