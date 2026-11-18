'use client';

import { useId, type ReactElement } from 'react';
import { useThemePalette } from '@/hooks/useThemePalette';
import { FILL_SCAN } from '@/lib/palette';

/** The scan of a chart's fills: the `<defs>` to draw once in its svg, and the fills' `mask`. */
export interface ScanFill {
  defs: ReactElement;
  /** The `mask` attribute of a fill (an area, a bar, a group of bars), never of a word. */
  mask: string;
}

/**
 * The scan of an SVG fill (readability R1, Gate F0), the SVG twin of the class `.scan-fill`: by
 * night a chart's areas and bars are cut by the scan, of every `FILL_SCAN.period` px one row
 * letting the ground through at `FILL_SCAN.strength`; its lines, ticks, labels and markers are
 * not. Null by day, when the material `fill-scan` is `none`: no mask and no defs, so the day's
 * chart is the one it was. The mask is in the svg's own user space, so a chart drawn 1:1 (its
 * viewBox its size in CSS px, as `Chart` and the home charts draw) keeps the rhythm at 3 CSS px
 * on the screen; a chart that scales its viewBox must not take it. Alpha only, no colour: 2 px
 * whole, then 1 px at `1 − strength`, as `fill-scan` is for an HTML fill.
 *
 * A component reads the theme here, not through a CSS variable, because a mask is referenced by
 * an id of its own chart, which a variable cannot carry: the material decides, as a canvas reads
 * its concrete values through `useThemePalette()`.
 */
export function useScanFill(): ScanFill | null {
  const { materials } = useThemePalette();
  // one id per chart, safe in a url(#…) reference whatever form React gives it
  const id = `scan-mask${useId().replace(/[^\w-]/g, '')}`;
  if (materials['fill-scan'] === 'none') return null;
  const { period, row, strength } = FILL_SCAN;
  return {
    mask: `url(#${id})`,
    defs: (
      <defs>
        <pattern id={`${id}-rows`} patternUnits="userSpaceOnUse" width={period} height={period}>
          <rect width={period} height={period - row} />
          <rect y={period - row} width={period} height={row} fillOpacity={1 - strength} />
        </pattern>
        <mask id={id} maskUnits="userSpaceOnUse" style={{ maskType: 'alpha' }}>
          <rect x="-10%" y="-10%" width="120%" height="120%" fill={`url(#${id}-rows)`} />
        </mask>
      </defs>
    ),
  };
}
