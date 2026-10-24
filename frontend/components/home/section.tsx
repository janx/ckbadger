'use client';

import type { ReactNode } from 'react';
import type { ChartBody } from '@/components/home/mini-chart';

/** A homepage section heading: unframed, 15 px / 500, sentence case. */
export const SECTION_HEADING_CLASS = 'text-text-bright m-0 text-[15px] font-medium';

/** The small-multiples grid: same-size charts, as many per row as fit at 220 px. */
export const SMALL_MULTIPLES_CLASS =
  'grid grid-cols-[repeat(auto-fit,minmax(220px,1fr))] gap-x-10 gap-y-8';

/** What a figure shows: its data is loading, failed, empty, or derived into a view. */
export type Derived<V> =
  | { state: 'loading' }
  | { state: 'error'; message: string }
  | { state: 'empty'; message: string }
  | { state: 'ready'; view: V };

/**
 * Derive a figure's view from its query. A failed fetch and a response that breaks its contract
 * (the derivation throws) both become an explicit error state, never a made-up chart; a response
 * with nothing to draw (`compute` returns null) is the empty state.
 */
export function derive<D, V>(
  query: { data: D | undefined; error: Error | null },
  compute: (data: D) => V | null,
  emptyMessage: string
): Derived<V> {
  if (query.error) return { state: 'error', message: `Failed to load: ${query.error.message}` };
  if (query.data === undefined) return { state: 'loading' };
  try {
    const view = compute(query.data);
    return view === null ? { state: 'empty', message: emptyMessage } : { state: 'ready', view };
  } catch (e) {
    return { state: 'error', message: e instanceof Error ? e.message : String(e) };
  }
}

/** The figure body for a derived view. */
export function bodyOf<V>(d: Derived<V>, chart: (view: V) => ReactNode): ChartBody {
  return d.state === 'ready' ? { state: 'ready', chart: chart(d.view) } : d;
}

/** The view, when there is one (for captions that show a headline). */
export function viewOf<V>(d: Derived<V>): V | null {
  return d.state === 'ready' ? d.view : null;
}
