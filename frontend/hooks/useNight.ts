'use client';

import { useEffect, useRef, useState, type RefObject } from 'react';
import { useRealtimeData, useRealtimeStore, type Block } from '@/hooks/useRealtimeStore';
import type { Grid } from '@/lib/night/bits';
import { createNight, type Night, type NightState } from '@/lib/night/night';

export interface NightLayout {
  grid: Grid;
  /** Half-resolution ink and trails on a phone. */
  half: boolean;
}

/** Where the grid stands at each width: under the sign on a phone, right of centre on a desk. */
export function nightLayout(width: number): NightLayout {
  if (width < 640) return { grid: { cx: 0.5, cy: 0.42, pitch: 12 }, half: true };
  if (width < 1024) return { grid: { cx: 0.62, cy: 0.48, pitch: 14 }, half: false };
  return { grid: { cx: 0.72, cy: 0.5, pitch: 18 }, half: false };
}

export type NightViewState = NightState | 'idle';

export interface UseNight {
  state: NightViewState;
  /** The latest block the realtime feed delivered; its hash was poured. */
  block: Block | null;
}

const RESIZE_SETTLE_MS = 200;

/**
 * Mounts the night on a canvas, pours the page's hash first (or the tip's, when the feed already
 * knows it) and every new block's after; honours reduced motion; rebuilds on a width change.
 */
export function useNight(
  canvasRef: RefObject<HTMLCanvasElement | null>,
  hash: string | null
): UseNight {
  const { latestBlock } = useRealtimeData(true);
  const [state, setState] = useState<NightViewState>('idle');
  const [layout, setLayout] = useState<NightLayout>(() =>
    nightLayout(typeof window === 'undefined' ? 1280 : window.innerWidth)
  );
  const [sizeKey, setSizeKey] = useState(0);
  const nightRef = useRef<Night | null>(null);
  const pouredRef = useRef<string | null>(null);

  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | null = null;
    const onResize = () => {
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => {
        const next = nightLayout(window.innerWidth);
        setLayout((current) =>
          current.half === next.half && current.grid.pitch === next.grid.pitch ? current : next
        );
        setSizeKey((k) => k + 1);
      }, RESIZE_SETTLE_MS);
    };
    window.addEventListener('resize', onResize);
    return () => {
      window.removeEventListener('resize', onResize);
      if (timer) clearTimeout(timer);
    };
  }, []);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const still = !!window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;
    const first = hash ?? useRealtimeStore.getState().latestBlock?.hash ?? null;
    pouredRef.current = first;
    const night = createNight(canvas, {
      hash: first,
      grid: layout.grid,
      half: layout.half,
      still,
      onState: setState,
    });
    nightRef.current = night;
    return () => {
      night.stop();
      nightRef.current = null;
      setState('idle');
    };
  }, [canvasRef, hash, layout, sizeKey]);

  useEffect(() => {
    const next = latestBlock?.hash;
    if (!next || next === pouredRef.current || !nightRef.current) return;
    pouredRef.current = next;
    nightRef.current.pour(next);
  }, [latestBlock]);

  return { state, block: latestBlock };
}
