'use client';

import { useEffect, useRef } from 'react';
import { useNight } from '@/hooks/useNight';
import type { Block } from '@/hooks/useRealtimeStore';
import { bitsOf, HASH32 } from '@/lib/night/bits';

interface ErrorNightProps {
  /** The hash this page is about, poured as the first grid; null waits for the first block. */
  hash: string | null;
  onBlock?: (block: Block) => void;
}

/**
 * The night: one WebGL canvas fed by the realtime blocks. While it cannot run (no WebGL2, a lost
 * context) the CSS night stands in: the same grid as 256 DOM squares and the afterglow.
 */
export function ErrorNight({ hash, onBlock }: ErrorNightProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const { state, block } = useNight(canvasRef, hash);
  useEffect(() => {
    if (block && onBlock) onBlock(block);
  }, [block, onBlock]);
  const down = state === 'lost' || state === 'failed';
  return (
    <div className="error-night" aria-hidden="true" data-night={state}>
      <canvas ref={canvasRef} className={down ? 'hidden' : undefined} />
      {down && <CssNight hash={block?.hash ?? hash} />}
    </div>
  );
}

/** The night without WebGL: the grid of the hash as DOM squares, a wash, the afterglow; nothing moves. */
export function CssNight({ hash }: { hash: string | null }) {
  const bits = hash && HASH32.test(hash) ? bitsOf(hash) : null;
  return (
    <div className="error-night-css" data-testid="css-night">
      <div className="error-night-haze" />
      <div className="error-night-glow" />
      {bits && (
        <div className="error-night-bits">
          {Array.from(bits, (bit, i) => (
            <i key={i} className={bit ? 'one' : 'zero'} />
          ))}
        </div>
      )}
    </div>
  );
}
