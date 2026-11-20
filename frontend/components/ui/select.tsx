'use client';

import { useId, useLayoutEffect, useRef, useState, type RefObject } from 'react';
import { FIELD_WELL } from '@/components/ui/text-field';
import { selectWellUnderLabel } from '@/lib/control-row-fit';
import { cn } from '@/lib/utils';

/** One choice of a select: the value it answers and the words it shows. */
export interface SelectOption<V extends string> {
  value: V;
  label: string;
}

interface SelectProps<V extends string> {
  /** The label it stands after (`sorted by`, `standard`), in the quiet ink. */
  label: string;
  /**
   * Its name for a screen reader where its label does not say which choice it is (a tab row's
   * narrow form stands after "showing": lane cd-fixes-controls, review-fix-logic-pages NOTE 4); the
   * label stays the word it stands after. Absent, the label names it.
   */
  name?: string;
  /** The id of the panel its choice shows, where it has one (a tab row's narrow form). */
  controls?: string;
  value: V;
  options: readonly SelectOption<V>[];
  onChange: (value: V) => void;
  className?: string;
}

/**
 * The width of the line an element stands in: its parent's content box, measured with a
 * ResizeObserver (which reports it when it starts observing and on every change); null until the
 * line has a width.
 */
function useLineWidth(ref: RefObject<HTMLElement | null>): number | null {
  const [width, setWidth] = useState<number | null>(null);
  useLayoutEffect(() => {
    const line = ref.current?.parentElement;
    if (!line) return;
    const ro = new ResizeObserver((entries) => {
      const w = entries[entries.length - 1].contentRect.width;
      setWidth((prev) => (w <= 0 || (prev !== null && Math.abs(prev - w) < 0.5) ? prev : w));
    });
    ro.observe(line);
    return () => ro.disconnect();
  }, [ref]);
  return width;
}

/**
 * A select (the controls board, gate D0′; UI review 2026-11-14, phase C): a real `<select>` in
 * the field's well (components/ui/text-field.tsx `FIELD_WELL`), 13 px, its chosen value in the
 * reading ink, the board's chevron after it in the quiet ink; it always stands after its label,
 * which is quiet and names it (`<label for>`). The list it opens is the browser's own.
 *
 * It never cuts its value (lane c2-finish, item 2): a native select is as wide as its widest
 * option and clips its chosen one, with no mark, where its well is narrower, so the well never
 * shrinks. The pair stands on one line where it fits the line it stands in and the well goes under
 * its label where it does not, by measurement (lib/control-row-fit.ts `selectWellUnderLabel`); an
 * option wider than a 360 px window's line even alone throws, a name for its caller to shorten.
 */
export function Select<V extends string>({
  label,
  name,
  controls,
  value,
  options,
  onChange,
  className,
}: SelectProps<V>) {
  const id = useId();
  const rootRef = useRef<HTMLSpanElement>(null);
  const linePx = useLineWidth(rootRef);
  if (label.trim() === '')
    throw new Error('Select: a select stands after its label, and this one has none');
  if (!options.some((option) => option.value === value))
    throw new Error(`Select "${label}": no option ${value}`);
  const under = selectWellUnderLabel(
    label,
    options.map((option) => option.label),
    linePx
  );
  return (
    <span
      ref={rootRef}
      data-well={under ? 'under' : 'after'}
      className={cn(
        under ? 'flex flex-col items-start gap-1.5' : 'inline-flex items-center gap-2.5',
        className
      )}
    >
      <label htmlFor={id} className="text-small text-text-dim whitespace-nowrap">
        {label}
      </label>
      <span className={cn(FIELD_WELL, 'relative shrink-0 px-0')}>
        <select
          id={id}
          aria-label={name}
          aria-controls={controls}
          value={value}
          onChange={(event) => onChange(event.target.value as V)}
          className="text-small text-text h-full shrink-0 appearance-none bg-transparent pl-2.5 pr-[25px] font-mono focus-visible:outline-none"
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
