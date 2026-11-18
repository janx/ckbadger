'use client';

import { useId } from 'react';
import { LINK_PAINT } from '@/lib/pipeline/encoding';
import type { ChainPaths, Orientation } from '@/lib/pipeline/layout';

interface ChainLineProps {
  paths: ChainPaths;
  /** The chain's direction: the tail fades to nothing along it (right in a row, down a column). */
  along: Orientation;
  /**
   * The window is pointed at and what lies outside it steps back: the plate-tone strap and NEXT's
   * dashed pair step back with the plates they join; the gilt never does.
   */
  dim: boolean;
}

const d = (p: string) => p || 'M0 0';

/**
 * The chain as one strap through the plates' heads (direction G, lib/pipeline/layout.ts
 * `chainPaths`): a filled rectangle across the whole gap between a block and its parent, in the
 * plate's own tone, so the chain reads as one piece; gilt between two plates inside the proposal
 * window, at full strength, at rest too; from NEXT (and from a block still landing) two dashed
 * lines in NEXT's outline paint, the only stroke in the chain. Past the last plate the strap runs
 * on as the tail, gilt while the window runs on past it, and fades to nothing along its length.
 * Under the pointer the plate-tone pieces step back with the plates they join (`dim`); the gilt
 * is a ground and never does.
 */
export function ChainLine({ paths, along, dim }: ChainLineProps) {
  // one id per canvas, safe in a url(#…) reference whatever form React gives it
  const id = `pipeline-tail${useId().replace(/[^\w-]/g, '')}`;
  const row = along === 'row';
  const fade = `url(#${id}-mask)`;
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
      <g data-dim={dim ? 'true' : 'false'}>
        <path data-path="solid" d={d(paths.solid)} style={{ fill: LINK_PAINT.solid.fill }} />
        <path
          data-path="tail"
          d={d(paths.tail)}
          mask={fade}
          style={{ fill: LINK_PAINT.solid.fill }}
        />
        <path
          data-path="pend"
          d={d(paths.pend)}
          fill="none"
          strokeWidth={1}
          strokeOpacity={LINK_PAINT.pend.strokeOpacity}
          strokeDasharray={LINK_PAINT.pend.dash}
          vectorEffect="non-scaling-stroke"
          style={{ stroke: LINK_PAINT.pend.stroke }}
        />
      </g>
      <path data-path="win" d={d(paths.win)} style={{ fill: LINK_PAINT.win.fill }} />
      <path
        data-path="tail-win"
        d={d(paths.tailWin)}
        mask={fade}
        style={{ fill: LINK_PAINT.win.fill }}
      />
    </g>
  );
}
