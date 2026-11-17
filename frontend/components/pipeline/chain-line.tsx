'use client';

import { useId } from 'react';
import { LINK_PAINT, tailFill } from '@/lib/pipeline/encoding';
import type { ChainPaths, Orientation } from '@/lib/pipeline/layout';

interface ChainLineProps {
  paths: ChainPaths;
  /** The drawn window runs on past the last plate on screen: the tail is gold. */
  tailOpen: boolean;
  /** The chain's direction: the tail fades to nothing along it (right in a row, down a column). */
  along: Orientation;
}

const d = (p: string) => p || 'M0 0';

/**
 * The chain: each link two bands of filled ticks across the gap between a block and its parent
 * (lib/pipeline/layout.ts `chainPaths`), no arrowheads and no strokes. The muted ink between two
 * blocks, at 0.7 from NEXT and from a block still landing; inside the proposal window the bands are
 * gold at full strength, so the window reads at rest with no mark of its own. Past the last plate
 * the two bands run on as the tail and fade to nothing along their length.
 */
export function ChainLine({ paths, tailOpen, along }: ChainLineProps) {
  // one id per canvas, safe in a url(#…) reference whatever form React gives it
  const id = `pipeline-tail${useId().replace(/[^\w-]/g, '')}`;
  const row = along === 'row';
  return (
    <g data-chain pointerEvents="none">
      <defs>
        {/* opaque at the last plate, nothing at the tail's end: an alpha mask, no colour */}
        <linearGradient id={`${id}-fade`} x1="0" y1="0" x2={row ? '1' : '0'} y2={row ? '0' : '1'}>
          <stop offset="0" stopOpacity={1} />
          <stop offset="1" stopOpacity={0} />
        </linearGradient>
        <mask id={`${id}-mask`} maskContentUnits="objectBoundingBox" style={{ maskType: 'alpha' }}>
          <rect width="1" height="1" fill={`url(#${id}-fade)`} />
        </mask>
      </defs>
      <path data-path="solid" d={d(paths.solid)} style={{ fill: LINK_PAINT.solid.fill }} />
      <path
        data-path="pend"
        d={d(paths.pend)}
        fillOpacity={LINK_PAINT.pend.fillOpacity}
        style={{ fill: LINK_PAINT.pend.fill }}
      />
      <path data-path="win" d={d(paths.win)} style={{ fill: LINK_PAINT.win.fill }} />
      <path
        data-path="tail"
        d={d(paths.tail)}
        mask={`url(#${id}-mask)`}
        style={{ fill: tailFill(tailOpen) }}
      />
    </g>
  );
}
