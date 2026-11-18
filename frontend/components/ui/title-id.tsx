'use client';

import { useLayoutEffect, useRef, useState } from 'react';
import { NoBreak } from '@/components/ui/no-break';
import { MONO_ADVANCE_EM } from '@/lib/type-metrics';

/** The two sizes an h1 id takes (U-09): the head where it fits, the row otherwise. */
export type TitleIdSize = 'head' | 'row';

/**
 * An id as a page's h1 (U-09; `docs/RESPONSIVE_LAYOUT.md` principle 4, the measured width
 * decides): the head form where the h1's measured line holds it whole, the row form otherwise. It
 * is the row form until the line is measured (the render before the layout effect, a hidden page,
 * jsdom), so a narrow h1 never shows the long form for a frame.
 *
 * The line is this block's width, the h1's content box (an h1 stands alone on its line in a page
 * head). A character's advance is 0.6 em of the h1's own font size plus its tracking, both read
 * from the computed style, since the h1 steps from 30 px to 26 px below md and is tracked −0.02 em
 * (globals.css): 17.4 px and 15.08 px. Measured before paint and again on every resize. `form`
 * writes the id at a size (a renderer takes a size, never a length).
 */
export function TitleId({ form }: { form: (size: TitleIdSize) => string }) {
  const ref = useRef<HTMLSpanElement>(null);
  const [size, setSize] = useState<TitleIdSize>('row');
  const head = form('head');
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const read = () => {
      const line = el.getBoundingClientRect().width;
      const style = window.getComputedStyle(el);
      const fontSize = parseFloat(style.fontSize);
      // computed letter-spacing is `normal` (no tracking) or a length in px
      const tracking = style.letterSpacing === 'normal' ? 0 : parseFloat(style.letterSpacing);
      // not laid out (no width, or no type to read) is no measurement: keep what there was
      if (!(line > 0) || !(fontSize > 0) || Number.isNaN(tracking)) return;
      const needed = head.length * (MONO_ADVANCE_EM * fontSize + tracking);
      setSize(line >= needed ? 'head' : 'row');
    };
    read();
    const ro = new ResizeObserver(read);
    ro.observe(el);
    return () => ro.disconnect();
  }, [head]);
  return (
    <span ref={ref} data-title-id={size} className="block">
      <NoBreak>{form(size)}</NoBreak>
    </span>
  );
}
