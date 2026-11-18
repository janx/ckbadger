'use client';

import { G, type WindowLabelGeom } from '@/lib/pipeline/layout';
import { T10C, T10G } from '@/components/pipeline/svg-text';

interface WindowLabelProps {
  /** Where `windowLane` put the words; null when no plate of the window is in view. */
  label: WindowLabelGeom | null;
  /** The window lane is hovered. */
  emphasis: boolean;
  /** A reward link uses the lane above the cards; the words step back meanwhile. */
  faded: boolean;
  text: string;
}

/**
 * The window's one line of words in the lane above the plates (a row; a column has no lane),
 * start-anchored where `windowLane` put it: at the left edge of the window's first plate in view,
 * or ending at the view's edge. Nothing else marks the window under the pointer: the gilt strap
 * says it (direction G).
 */
export function WindowLabel({ label, emphasis, faded, text }: WindowLabelProps) {
  if (label === null) return null;
  return (
    <text
      data-window-label
      x={label.x0.toFixed(1)}
      y={G.winY + 3.5}
      textAnchor="start"
      opacity={faded ? 0.15 : 1}
      pointerEvents="none"
      className={emphasis ? T10C : T10G}
    >
      {text}
    </text>
  );
}
