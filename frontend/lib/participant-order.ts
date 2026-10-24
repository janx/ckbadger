/**
 * The order the homepage feed lists a transaction's participants in, and how many it shows.
 * Ordering is by exact shannons: what was paid (negative), then what was received (positive),
 * then the named parties whose position did not change — each largest |CKB delta| first.
 */

/** Up to this many participants are all shown; beyond it, the largest five and a count. */
export const PARTICIPANT_LIMIT = 6;
const SHOWN_WHEN_FOLDED = 5;

interface HasCkbDelta {
  ckbDelta: string;
}

function rank(delta: bigint): number {
  return delta < BigInt(0) ? 0 : delta > BigInt(0) ? 1 : 2;
}

function magnitude(delta: bigint): bigint {
  return delta < BigInt(0) ? -delta : delta;
}

/** Negative first, then positive, then zero; within each, larger |delta| first; ties keep order. */
export function orderParticipants<T extends HasCkbDelta>(participants: T[]): T[] {
  const keyed = participants.map((p, i) => ({ p, i, d: BigInt(p.ckbDelta) }));
  keyed.sort((a, b) => {
    const r = rank(a.d) - rank(b.d);
    if (r !== 0) return r;
    const ma = magnitude(a.d);
    const mb = magnitude(b.d);
    if (ma !== mb) return ma > mb ? -1 : 1;
    return a.i - b.i;
  });
  return keyed.map((k) => k.p);
}

/**
 * The participants to draw: all of them up to PARTICIPANT_LIMIT (or when expanded); otherwise the
 * five with the largest |CKB delta|, in display order, and how many are folded away. Never a silent
 * omission: `hidden` is what the "+N participants" control reveals.
 */
export function participantsToShow<T extends HasCkbDelta>(
  participants: T[],
  expanded: boolean
): { shown: T[]; hidden: number } {
  const ordered = orderParticipants(participants);
  if (expanded || ordered.length <= PARTICIPANT_LIMIT) return { shown: ordered, hidden: 0 };
  const largest = new Set(
    participants
      .map((p, i) => ({ p, i, m: magnitude(BigInt(p.ckbDelta)) }))
      .sort((a, b) => (a.m === b.m ? a.i - b.i : a.m > b.m ? -1 : 1))
      .slice(0, SHOWN_WHEN_FOLDED)
      .map((x) => x.p)
  );
  return {
    shown: ordered.filter((p) => largest.has(p)),
    hidden: ordered.length - SHOWN_WHEN_FOLDED,
  };
}
