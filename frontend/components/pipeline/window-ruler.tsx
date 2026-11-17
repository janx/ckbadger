'use client';

import { tok } from '@/lib/pipeline/encoding';
import { G, type RulerGeom } from '@/lib/pipeline/layout';
import { T10C, T10G } from '@/components/pipeline/svg-text';

interface WindowRulerProps {
  ruler: RulerGeom;
  /** The window lane is hovered. */
  emphasis: boolean;
  /** A reward link uses the lane above the cards; the ruler steps back meanwhile. */
  faded: boolean;
}

/**
 * The hovered window: two ticks and the dimension line between them (a row), or ticks across the
 * gaps (a column); the rest is in the hover. `d` comes from the scene's axis. At rest the window
 * draws nothing of its own: its gold bands say it (chain-line.tsx).
 */
export function WindowTicks({ d, faded }: { d: string; faded: boolean }) {
  return (
    <path
      data-ruler-ticks
      d={d || 'M0 0'}
      fill="none"
      strokeWidth={1}
      strokeOpacity={0.9}
      opacity={faded ? 0.15 : 1}
      vectorEffect="non-scaling-stroke"
      pointerEvents="none"
      style={{ stroke: tok('gold') }}
    />
  );
}

/** The window's one line of text, where `windowRuler` put it: in the line's break, or beside it. */
export function WindowLabel({ ruler, emphasis, faded, text }: WindowRulerProps & { text: string }) {
  if (ruler.label === null) return null;
  return (
    <text
      data-window-label
      x={ruler.label.x.toFixed(1)}
      y={G.winY + 3.5}
      textAnchor="middle"
      opacity={faded ? 0.15 : 1}
      pointerEvents="none"
      className={emphasis ? T10C : T10G}
    >
      {text}
    </text>
  );
}
