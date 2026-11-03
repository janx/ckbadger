'use client';

import { useEffect, useRef, useState } from 'react';
import { usePrefersReducedMotion } from '@/hooks/usePrefersReducedMotion';

/** How long the flare lasts: the `.phosphor-flare` animation in app/globals.css (1.2 s). */
export const PHOSPHOR_MS = 1200;

/**
 * Phosphor behaviour (material pass v2, move 4): a figure that updates live flares to the neon
 * white and decays to its idle state over 1.2 s. Returns the class to put on the figure —
 * `phosphor-flare` for 1.2 s after `value` changes, never on mount — and nothing under reduced
 * motion. A figure's arrival (nothing, while its data loads, then a value) and its departure (a
 * value, then nothing) are not changes: the flare is for a live update of a figure already shown. `value` is compared with `Object.is`, so
 * pass the figure's primitive (a string, number or bigint), not an element.
 */
export function usePhosphor(
  value: string | number | bigint | null | undefined
): string | undefined {
  const reduced = usePrefersReducedMotion();
  const previous = useRef(value);
  const [lit, setLit] = useState(false);
  useEffect(() => {
    if (Object.is(previous.current, value)) return;
    // nothing → a value is the figure's arrival; a value → nothing is its departure: no flare
    const arrival = previous.current === undefined || previous.current === null;
    const departure = value === undefined || value === null;
    previous.current = value;
    if (arrival || departure || reduced) return;
    setLit(true);
    // A change during a flare keeps the class lit for 1.2 s after the latest change.
    const timer = setTimeout(() => setLit(false), PHOSPHOR_MS);
    return () => clearTimeout(timer);
  }, [value, reduced]);
  return lit && !reduced ? 'phosphor-flare' : undefined;
}
