'use client';

import {
  createContext,
  useContext,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type ReactNode,
  type RefObject,
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
 * `TapOrHoverHintProvider` (mounted at the route root) knows every mounted hint by its element; the
 * first in document order renders its words (DY-11: the first registered followed data arrival, so
 * the DAO page's hint sat on a different chart per load). A hint that does not render its words
 * keeps an empty, hidden element in its place, so its place in the document is known. When a page
 * unmounts its hints leave the registry, so the next page's first hint is first again.
 */
class HintRegistry {
  private readonly mounted = new Map<string, Element>();
  private readonly listeners = new Set<() => void>();

  register(id: string, element: Element): () => void {
    this.mounted.set(id, element);
    this.emit();
    return () => {
      this.mounted.delete(id);
      this.emit();
    };
  }

  /** The id of the mounted hint that comes first in the document; null while none is. */
  first(): string | null {
    let best: [string, Element] | null = null;
    for (const entry of this.mounted) {
      if (!entry[1].isConnected)
        throw new Error(`tap-or-hover registry: hint ${entry[0]} is not in the document`);
      if (!best || entry[1].compareDocumentPosition(best[1]) & Node.DOCUMENT_POSITION_FOLLOWING)
        best = entry;
    }
    return best ? best[0] : null;
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

/**
 * Whether this hint is the page's one: the first in the document under the provider, else always.
 * `place` is the hint's element, rendered whether or not it says its words.
 */
function useIsFirstHint(place: RefObject<HTMLSpanElement | null>): boolean {
  const registry = useContext(HintRegistryContext);
  const id = useId();
  useLayoutEffect(() => {
    if (!registry) return undefined;
    if (!place.current) throw new Error('tap-or-hover hint: its element is not mounted');
    return registry.register(id, place.current);
  }, [registry, id, place]);
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
  /**
   * Led by " · " after the sentence it follows (PB-23): the dot belongs to the hint, drawn only
   * when the hint is, and kept on its first word's line (a no-break space after it).
   */
  dotted?: boolean;
  className?: string;
}

/**
 * The touch hint as an inline element: "tap or hover a day", in the dim ink wherever it stands (a
 * hint is information, never the ghost: R4). Under `TapOrHoverHintProvider` only the page's first
 * hint in the document says its words; the later ones keep an empty, hidden element in their
 * place.
 */
export function TapOrHoverHint({ what, capital, dotted = false, className }: TapOrHoverHintProps) {
  const place = useRef<HTMLSpanElement>(null);
  const first = useIsFirstHint(place);
  // a later hint keeps only its place: an empty element that draws no box
  if (!first) return <span ref={place} data-hint-place="" hidden />;
  return (
    // a hint is a note: quiet by rule (R3)
    <span ref={place} data-hint="tap-or-hover" className={cn('text-text-dim', className)}>
      {dotted && ' ·\u00a0'}
      {tapOrHover(what, { capital })}
    </span>
  );
}
