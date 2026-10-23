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

/** The chain through the card headers; each gap carries an arrow to the parent. */
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
        data-path="arrows"
        d={d(paths.arrows)}
        {...common}
        strokeOpacity={0.85}
        strokeLinecap="round"
        strokeLinejoin="round"
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
        strokeOpacity={0.9}
        strokeWidth={windowEmphasis ? 2 : 1.5}
        style={{ stroke: tok('gold') }}
      />
      <path
        data-path="winArrows"
        d={d(paths.winArrows)}
        {...common}
        strokeLinecap="round"
        strokeLinejoin="round"
        style={{ stroke: tok('gold') }}
      />
      <path
        data-path="tail"
        d={d(paths.tail)}
        {...common}
        strokeDasharray="1 3"
        strokeOpacity={tailOpen ? 0.7 : 0.25}
        style={{ stroke: tailOpen ? tok('gold') : tok('jade') }}
      />
    </g>
  );
}
