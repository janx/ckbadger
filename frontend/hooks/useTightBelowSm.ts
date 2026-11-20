'use client';

import { useFromSm } from '@/hooks/useFromMd';
import type { ShortSize } from '@/lib/shorten';

/**
 * The size of an id in a place that is narrow only on a phone (R9, `docs/RESPONSIVE_LAYOUT.md`,
 * "Formats": `tight` only below `sm`): a list's phone row that is also drawn in a desktop column
 * (a holders list in its column at 1280, a list whose table needs more than its container), and a
 * tile under 200 px only on a phone. Below `sm` the tight size; from `sm` the row size, the form
 * the page's tables and prose give the same id. Follows the viewport across the breakpoint
 * (`useFromSm`); the row size before there is a viewport to ask.
 */
export function useTightBelowSm(): ShortSize {
  return useFromSm() ? 'row' : 'tight';
}
