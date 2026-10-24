'use client';

import { useLayoutEffect, useRef, useState, type RefObject } from 'react';

/**
 * The rendered width of an element, measured before paint and kept current with a ResizeObserver;
 * null until the element has a width. Charts use it to draw 1:1 (a viewBox as wide as the box),
 * so their 10 px axis text stays 10 px at every column width instead of scaling with the SVG.
 */
export function useElementWidth<T extends HTMLElement>(): [RefObject<T | null>, number | null] {
  const ref = useRef<T>(null);
  const [width, setWidth] = useState<number | null>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const read = (w: number) =>
      setWidth((prev) => (w <= 0 || (prev !== null && Math.abs(prev - w) < 0.5) ? prev : w));
    read(el.getBoundingClientRect().width);
    const ro = new ResizeObserver((entries) => read(entries[entries.length - 1].contentRect.width));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  return [ref, width];
}
