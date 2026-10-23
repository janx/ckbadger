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

/** Two ticks at the ends of the proposal window and one word of label; the rest is in the hover. */
export function WindowTicks({ ruler, faded }: Omit<WindowRulerProps, 'emphasis'>) {
  const d = ruler.ticks
    .map((x) => `M${x.toFixed(1)} ${G.cardTop - 12}V${G.cardTop + G.headH}`)
    .join('');
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

export function WindowLabel({ ruler, emphasis, faded }: WindowRulerProps) {
  if (ruler.labelX === null) return null;
  return (
    <text
      data-window-label
      x={ruler.labelX.toFixed(1)}
      y={G.cardTop - 4}
      opacity={faded ? 0.15 : 1}
      pointerEvents="none"
      className={emphasis ? T10C : T10G}
    >
      proposal window
    </text>
  );
}
