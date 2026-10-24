'use client';

import { tok } from '@/lib/pipeline/encoding';
import type { ChainPaths } from '@/lib/pipeline/layout';

interface ChainLineProps {
  paths: ChainPaths;
  /** The window runs past the last card on screen: the dotted tail turns gold. */
  tailOpen: boolean;
  /** The window lane is hovered: its line thickens. */
  windowEmphasis: boolean;
}

const d = (p: string) => p || 'M0 0';

/**
 * The chain through the card headers: a plain line, no arrowheads (the age fade gives it its
 * direction). Dashed before the tip, solid after it; inside the proposal window the links are gold,
 * so the window reads at rest without a label.
 */
export function ChainLine({ paths, tailOpen, windowEmphasis }: ChainLineProps) {
  const common = { fill: 'none', vectorEffect: 'non-scaling-stroke' as const, strokeWidth: 1 };
  return (
    <g data-chain pointerEvents="none">
      <path
        data-path="solid"
        d={d(paths.solid)}
        {...common}
        strokeOpacity={0.55}
        style={{ stroke: tok('jade') }}
      />
      <path
        data-path="pend"
        d={d(paths.pend)}
        {...common}
        strokeOpacity={0.55}
        strokeDasharray="3 3"
        style={{ stroke: tok('jade') }}
      />
      <path
        data-path="win"
        d={d(paths.win)}
        {...common}
        strokeOpacity={0.85}
        strokeWidth={windowEmphasis ? 2 : 1.5}
        style={{ stroke: tok('gold') }}
      />
      <path
        data-path="tail"
        d={d(paths.tail)}
        {...common}
        strokeDasharray="1 3"
        strokeOpacity={tailOpen ? 0.7 : 0.25}
        style={{ stroke: tok(tailOpen ? 'gold' : 'jade') }}
      />
    </g>
  );
}
