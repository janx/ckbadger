'use client';

import { useCallback, useEffect, useState, type KeyboardEvent } from 'react';

/** The props that make one element a hover/tap target of a pinned readout. */
export interface PinnedTargetProps {
  tabIndex: 0;
  role: 'button';
  'aria-pressed': boolean;
  onMouseEnter: () => void;
  onMouseLeave: () => void;
  onFocus: () => void;
  onBlur: () => void;
  onClick: () => void;
  onKeyDown: (event: KeyboardEvent) => void;
}

export interface PinnedReadoutOptions {
  /**
   * A hover outlasts the pointer (and focus) while nothing is pinned, until another target is
   * hovered: for a figure whose panel holds links the pointer travels to. Once a target is pinned,
   * leaving a hovered one shows the pin again.
   */
  sticky?: boolean;
}

export interface PinnedReadout<T> {
  /** What the readout shows: the hovered (or focused) target, else the pinned one. */
  active: T | null;
  /** The target a tap pinned; it stays until a second tap on it or Escape. */
  pinned: T | null;
  /** Whether `active` is the pinned target (the readout then says "tapped"). */
  showingPin: boolean;
  hover: (target: T | null) => void;
  /** A tap: pins the target, or clears the pin when it is the pinned one. */
  toggle: (target: T) => void;
  clear: () => void;
  isPinned: (target: T) => boolean;
  /** Hover, focus, click, Enter and Space wired to one target. */
  bind: (target: T) => PinnedTargetProps;
}

/**
 * The shared touch rule (`docs/RESPONSIVE_LAYOUT.md`, Touch; plan R6): anything a hover reveals is
 * also reachable by a tap that pins its readout under the figure. A hover shows a target while the
 * pointer or focus is on it; a tap pins it; a second tap on the pinned target, or Escape, clears it.
 *
 * On a touch screen a tap arrives as an emulated mouseenter followed by the click, and the emulated
 * hover stays on the element until the next tap elsewhere; so clearing a pin also clears the hover,
 * or the second tap would leave the reading on screen. `keyOf` names a target, so a fresh object
 * for the same target counts as the same target.
 */
export function usePinnedReadout<T>(
  keyOf: (target: T) => string = String,
  { sticky = false }: PinnedReadoutOptions = {}
): PinnedReadout<T> {
  const [hovered, setHovered] = useState<T | null>(null);
  const [pinned, setPinned] = useState<T | null>(null);
  const same = (a: T | null, b: T) => a !== null && keyOf(a) === keyOf(b);
  const clear = useCallback(() => {
    setPinned(null);
    setHovered(null);
  }, []);
  const toggle = (target: T) => {
    if (same(pinned, target)) clear();
    else setPinned(target);
  };
  useEffect(() => {
    if (pinned === null) return;
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (event.key === 'Escape') clear();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [pinned, clear]);
  const active = hovered ?? pinned;
  const isPinned = (target: T) => same(pinned, target);
  const leave = () => {
    if (!sticky || pinned !== null) setHovered(null);
  };
  return {
    active,
    pinned,
    showingPin: active !== null && pinned !== null && keyOf(active) === keyOf(pinned),
    hover: setHovered,
    toggle,
    clear,
    isPinned,
    bind: (target) => ({
      tabIndex: 0,
      role: 'button',
      'aria-pressed': isPinned(target),
      onMouseEnter: () => setHovered(target),
      onMouseLeave: leave,
      onFocus: () => setHovered(target),
      onBlur: leave,
      onClick: () => toggle(target),
      onKeyDown: (event) => {
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault();
          toggle(target);
        }
      },
    }),
  };
}
