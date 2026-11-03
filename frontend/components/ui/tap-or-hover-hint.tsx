'use client';

import {
  createContext,
  useContext,
  useId,
  useLayoutEffect,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from 'react';
import { cn } from '@/lib/utils';

/**
 * The one wording of the touch hint (plan R6; `docs/RESPONSIVE_LAYOUT.md`, Touch): anything a hover
 * reveals is also reachable by a tap, so a hint never asks for a hover alone. Every hint in the
 * app is built here, so they all read the same (`__tests__/lib/tap-or-hover-wording.test.ts`).
 */
const WORDS = 'tap or hover';

export interface TapOrHoverOptions {
  /** Start a sentence: "Tap or hover a day." */
  capital?: boolean;
}

/**
 * The hint as text — "tap or hover a day", or "Tap or hover a day." with `capital` — for a string
 * (a `title`, an idle line passed as a prop). In JSX text render `TapOrHoverHint`.
 */
export function tapOrHover(what?: string, { capital = false }: TapOrHoverOptions = {}): string {
  const words = capital ? `${WORDS[0].toUpperCase()}${WORDS.slice(1)}` : WORDS;
  if (what === undefined) return words;
  if (what.trim() === '') throw new Error('tap-or-hover hint: an empty target');
  return `${words} ${what}`;
}

/**
 * Material pass v2 (move 5): "tap or hover" appears once per page. The registry under
 * `TapOrHoverHintProvider` (mounted at the route root) orders every mounted hint by the moment it
 * first registered; only the earliest still mounted renders its words. A hint keeps its place
 * across React's strict-mode remount (the ordinal is kept by its stable id), and when a page
 * unmounts its hints leave the registry, so the next page's first hint is first again.
 */
class HintRegistry {
  private seq = 0;
  private readonly ordinal = new Map<string, number>();
  private readonly mounted = new Set<string>();
  private readonly listeners = new Set<() => void>();

  register(id: string): () => void {
    if (!this.ordinal.has(id)) this.ordinal.set(id, this.seq++);
    this.mounted.add(id);
    this.emit();
    return () => {
      this.mounted.delete(id);
      this.emit();
    };
  }

  /** The id of the earliest-registered hint still mounted; null while none is. */
  first(): string | null {
    let best: string | null = null;
    let bestOrdinal = Infinity;
    for (const id of this.mounted) {
      const n = this.ordinal.get(id);
      if (n === undefined)
        throw new Error(`tap-or-hover registry: ${id} mounted without an ordinal`);
      if (n < bestOrdinal) {
        best = id;
        bestOrdinal = n;
      }
    }
    return best;
  }

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  private emit(): void {
    for (const listener of this.listeners) listener();
  }
}

/** No provider (a component rendered on its own, as in a unit test): every hint renders. */
const HintRegistryContext = createContext<HintRegistry | null>(null);

export function TapOrHoverHintProvider({ children }: { children: ReactNode }) {
  const [registry] = useState(() => new HintRegistry());
  return <HintRegistryContext.Provider value={registry}>{children}</HintRegistryContext.Provider>;
}

const noSubscribe = () => () => {};

/** Whether this hint is the page's one: the first registered under the provider, else always. */
function useIsFirstHint(): boolean {
  const registry = useContext(HintRegistryContext);
  const id = useId();
  useLayoutEffect(() => (registry ? registry.register(id) : undefined), [registry, id]);
  const first = useSyncExternalStore(
    registry ? registry.subscribe : noSubscribe,
    () => (registry ? registry.first() : id),
    () => id
  );
  return first === id;
}

export interface TapOrHoverHintProps extends TapOrHoverOptions {
  /** What the reader taps or hovers, e.g. "a day" or "for its name"; omitted, the bare words. */
  what?: string;
  className?: string;
}

/**
 * The touch hint as an inline element: "tap or hover a day". Under `TapOrHoverHintProvider` only
 * the page's first mounted hint renders; the later ones render nothing.
 */
export function TapOrHoverHint({ what, capital, className }: TapOrHoverHintProps) {
  const first = useIsFirstHint();
  if (!first) return null;
  return (
    <span data-hint="tap-or-hover" className={cn(className)}>
      {tapOrHover(what, { capital })}
    </span>
  );
}
