'use client';

import { useId } from 'react';
import type { OrphanedBlock } from '@/lib/api';
import { FORK_COLORS } from '@/lib/design-tokens';
import { formatBlockNumber } from '@/lib/format-block';

const XS = [6, 56, 106, 156];

/**
 * A fork in one glyph (Forks V67): the spine from the fork point, the dropped block(s) dashed
 * above it, and a jade curve to the block that took the first one as an uncle.
 */
export function ForkGlyph({
  forkPoint,
  depth,
  uncleIn,
  fate,
}: {
  forkPoint: number;
  depth: number;
  uncleIn: number | null;
  fate: OrphanedBlock['fate'];
}) {
  const marker = useId().replace(/:/g, '');
  if (!Number.isSafeInteger(depth) || depth < 1)
    throw new Error(`Fork ${formatBlockNumber(forkPoint)}: invalid depth ${depth}`);
  if (uncleIn !== null && uncleIn < forkPoint + 2)
    throw new Error(
      `Fork ${formatBlockNumber(forkPoint)}: uncle inclusion ${formatBlockNumber(uncleIn)} is not after the dropped block`
    );
  const dropped = Math.min(depth, 3);
  const cameBack = fate === 'canonical_now';
  const slot = uncleIn === null ? null : Math.min(uncleIn - forkPoint, 3);
  return (
    <svg
      viewBox="0 0 210 44"
      width="210"
      height="44"
      aria-hidden="true"
      className="block overflow-visible"
    >
      <defs>
        <marker
          id={marker}
          viewBox="0 0 6 6"
          refX="5"
          refY="3"
          markerWidth="6"
          markerHeight="6"
          orient="auto"
        >
          <path d="M0 0L6 3L0 6Z" fill={FORK_COLORS.uncleLink} />
        </marker>
      </defs>
      <line x1={XS[0] + 9} x2={XS[3] + 9} y1="33" y2="33" stroke={FORK_COLORS.spine} />
      {XS.map((x, k) => (
        <rect
          key={k}
          data-slot={k}
          x={x}
          y="26"
          width="18"
          height="14"
          rx="2"
          fill={FORK_COLORS.tileFill}
          stroke={k === 0 ? FORK_COLORS.ink : FORK_COLORS.dim}
          strokeOpacity={k === 0 ? 1 : 0.6}
        />
      ))}
      {cameBack ? (
        <g data-came-back>
          <rect
            x={XS[1]}
            y="3"
            width="18"
            height="14"
            rx="2"
            fill="none"
            stroke={FORK_COLORS.tick}
            strokeDasharray="2 2"
          />
          <path d={`M${XS[1] + 9} 17 V26`} stroke={FORK_COLORS.ink} strokeDasharray="2 2" />
        </g>
      ) : (
        <>
          {Array.from({ length: dropped }, (_, k) => (
            <rect
              key={k}
              data-dropped
              x={XS[1 + k]}
              y="3"
              width="18"
              height="14"
              rx="2"
              fill="none"
              stroke={FORK_COLORS.dropped}
              strokeDasharray="2 2"
            />
          ))}
          <path
            d={`M${XS[0] + 18} 30 C${XS[0] + 30} 30 ${XS[1] - 10} 10 ${XS[1]} 10`}
            fill="none"
            stroke={FORK_COLORS.tick}
          />
          {slot !== null && (
            <path
              data-uncle-link
              data-uncle-slot={slot}
              d={`M${XS[dropped] + 18} 10 C${XS[slot] + 9} 10 ${XS[slot] + 9} 14 ${XS[slot] + 9} 26`}
              fill="none"
              stroke={FORK_COLORS.uncleLink}
              strokeWidth="1.2"
              markerEnd={`url(#${marker})`}
            />
          )}
        </>
      )}
    </svg>
  );
}
