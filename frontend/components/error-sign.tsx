'use client';

import { useCallback, useEffect, useRef, useState, type PointerEvent } from 'react';
import { clsx } from 'clsx';
import { tapOrHover } from '@/components/ui/tap-or-hover-hint';

/** One thought from the worldview cut into three tubes: two lit, the third never lit. */
export type SignLines = readonly [string, string, string];

interface ErrorSignProps {
  lines: SignLines;
  /** The other readings of the same thought; tap a lit line to cycle them. The first is `lines`. */
  readings?: readonly SignLines[];
  /** The error page: the lit tubes dip at random every few seconds. */
  brownout?: boolean;
  /** Changes with every block the night took: the sign flickers 300 ms after the grid lands. */
  blockKey?: string | null;
  /** The block that ended an epoch: the dark line is lit for one block. */
  epochTurn?: boolean;
  /** Hold the dark line for two seconds: it becomes the only light, for four. */
  onInverted?: (inverted: boolean) => void;
}

const WARM_START_MS = 300;
const WARM_STEP_MS = 320;
const WARM_MS = 1400;
const FLICK_AFTER_MS = 300;
const HOLD_MS = 2000;
const INVERT_MS = 4000;
const HUES = ['jade', 'aqua'] as const;

function reducedMotion(): boolean {
  return (
    typeof window !== 'undefined' &&
    !!window.matchMedia?.('(prefers-reduced-motion: reduce)').matches
  );
}

function retrigger(element: HTMLElement, className: string): void {
  element.classList.remove(className);
  void element.offsetWidth;
  element.classList.add(className);
}

type Pair = readonly [boolean, boolean];
const withIndex = (pair: Pair, i: number, value: boolean): Pair =>
  i === 0 ? [value, pair[1]] : [pair[0], value];

export function ErrorSign({
  lines,
  readings,
  brownout = false,
  blockKey,
  epochTurn = false,
  onInverted,
}: ErrorSignProps) {
  const key = lines.join('\u0001');
  const linesRef = useRef(lines);
  linesRef.current = lines;
  const [shown, setShown] = useState<SignLines>(lines);
  const [reading, setReading] = useState(0);
  const [lit, setLit] = useState<Pair>(() => (reducedMotion() ? [true, true] : [false, false]));
  const [warm, setWarm] = useState<Pair>([false, false]);
  const signRef = useRef<HTMLHeadingElement>(null);
  const darkRef = useRef<HTMLSpanElement>(null);
  const holdRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const releaseRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const previousBlock = useRef<string | null | undefined>(undefined);

  // the words follow the page (an entity lookup may change them)
  useEffect(() => {
    setShown(linesRef.current);
    setReading(0);
  }, [key]);

  // the warm-up: each lit tube stutters on in turn; the dead one never
  useEffect(() => {
    if (reducedMotion()) {
      setLit([true, true]);
      setWarm([false, false]);
      return;
    }
    setLit([false, false]);
    const timers: ReturnType<typeof setTimeout>[] = [];
    for (const i of [0, 1]) {
      const at = WARM_START_MS + i * WARM_STEP_MS;
      timers.push(setTimeout(() => setWarm((w) => withIndex(w, i, true)), at));
      timers.push(
        setTimeout(() => {
          setWarm((w) => withIndex(w, i, false));
          setLit((l) => withIndex(l, i, true));
        }, at + WARM_MS)
      );
    }
    return () => timers.forEach(clearTimeout);
  }, [shown]);

  // a block: the sign flickers (not for the block already known when the sign was lit)
  useEffect(() => {
    if (previousBlock.current === undefined) {
      previousBlock.current = blockKey ?? null;
      return;
    }
    if (!blockKey || blockKey === previousBlock.current) return;
    previousBlock.current = blockKey;
    if (reducedMotion()) return;
    const timer = setTimeout(() => {
      if (signRef.current) retrigger(signRef.current, 'sign-flick');
    }, FLICK_AFTER_MS);
    return () => clearTimeout(timer);
  }, [blockKey]);

  // the epoch turn: the one moment the thing that is not, is
  useEffect(() => {
    if (!epochTurn || reducedMotion() || !darkRef.current) return;
    retrigger(darkRef.current, 'once');
  }, [epochTurn, blockKey]);

  const stopHold = useCallback(() => {
    if (holdRef.current) {
      clearTimeout(holdRef.current);
      holdRef.current = null;
    }
  }, []);
  useEffect(
    () => () => {
      stopHold();
      if (releaseRef.current) clearTimeout(releaseRef.current);
    },
    [stopHold]
  );

  const cycle = () => {
    if (!readings || readings.length < 2) return;
    const next = (reading + 1) % readings.length;
    setReading(next);
    setShown(readings[next]);
  };
  const startHold = (event: PointerEvent<HTMLSpanElement>) => {
    event.preventDefault();
    stopHold();
    holdRef.current = setTimeout(() => {
      holdRef.current = null;
      onInverted?.(true);
      releaseRef.current = setTimeout(() => {
        releaseRef.current = null;
        onInverted?.(false);
      }, INVERT_MS);
    }, HOLD_MS);
  };
  const cycles = !!readings && readings.length > 1;

  return (
    <h1 ref={signRef} data-sign className={clsx('sign', brownout && 'sign-brownout')}>
      {shown.map((line, i) =>
        i < 2 ? (
          <span
            key={i}
            data-tube
            className={clsx('neon on', HUES[i], warm[i] && 'warm', lit[i] && 'lit')}
            style={cycles ? { cursor: 'pointer' } : undefined}
            title={cycles ? 'another reading' : undefined}
            onClick={cycles ? cycle : undefined}
          >
            {line}
          </span>
        ) : (
          <span
            key={i}
            ref={darkRef}
            data-tube
            className="neon off"
            title={tapOrHover()}
            onPointerDown={startHold}
            onPointerUp={stopHold}
            onPointerLeave={stopHold}
            onPointerCancel={stopHold}
          >
            {line}
          </span>
        )
      )}
    </h1>
  );
}
