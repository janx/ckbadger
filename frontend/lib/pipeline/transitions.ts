import type {
  PipelineBlock,
  PipelinePool,
  PipelineProposalState,
  PipelineSnapshot,
} from '@/lib/api';

/**
 * What changed between two observed pipeline snapshots, by hash. Only observed state changes become
 * events; nothing is predicted or interpolated. The motion layer turns events into animation.
 */

export type TransitionMode =
  /** First snapshot: draw the final state, no animation. */
  | 'boot'
  /** The tip advanced by exactly one block on top of the old tip: full choreography. */
  | 'mined'
  /** Several blocks at once (catching up) or a reorg: everything just settles into place. */
  | 'settle'
  /** Same tip: pool changes only. */
  | 'update';

/** `at` = ms after the transition starts at which the visible change happens. */
export type TransitionEvent =
  | { kind: 'arrive'; hash: string; at: number }
  /** Appears without an observed arrival (it was beyond a truncated list, or never in the pool). */
  | { kind: 'reveal'; hash: string; at: number }
  | { kind: 'propose'; hash: string; rank: number; at: number }
  | { kind: 'commit'; hash: string; block: string; number: number; rank: number; at: number }
  | { kind: 'return'; hash: string; rank: number; at: number }
  /** Left a complete pool list without being committed in any listed block. */
  | { kind: 'drop'; hash: string; at: number }
  /** Left a list whose completeness cannot prove a drop (truncated pool, omitted block txs). */
  | { kind: 'hide'; hash: string; at: number }
  | { kind: 'retire'; block: string; number: number; at: number }
  | { kind: 'orphan'; block: string; number: number; at: number }
  | { kind: 'enter'; block: string; number: number; at: number }
  | {
      kind: 'mined';
      block: string;
      number: number;
      cellbase: string;
      contact: number;
      payee: { number: number; hash: string; cellbase: string } | null;
    }
  | { kind: 'settle' }
  | {
      kind: 'resolve';
      block: string;
      shortId: string;
      uncleIndex: number | null;
      from: PipelineProposalState;
      to: PipelineProposalState;
      at: number;
    };

export interface Transition {
  mode: TransitionMode;
  events: TransitionEvent[];
  /** When the committed txs land in the new block (mined mode; 0 otherwise). */
  contact: number;
  /** When the new block joins the chain and the window moves (mined mode; 0 otherwise). */
  settleAt: number;
}

/** Choreography timing of the prototype's mine(): the last commit / proposal lands at contact. */
export function contactTime(commits: number, proposals: number): number {
  return Math.max(
    commits ? 40 + Math.min((commits - 1) * 16, 420) + 720 : 360,
    proposals ? 120 + Math.min((proposals - 1) * 12, 300) + 560 : 0
  );
}

export const SETTLE_AFTER_CONTACT = 380;

type TxPlace =
  | { stage: 'pending' }
  | { stage: 'proposed' }
  | { stage: 'committed'; block: string; number: number };

function places(s: PipelineSnapshot): Map<string, TxPlace> {
  const out = new Map<string, TxPlace>();
  for (const t of s.pool?.pending ?? []) out.set(t.hash, { stage: 'pending' });
  for (const t of s.pool?.proposed ?? []) out.set(t.hash, { stage: 'proposed' });
  for (const b of s.blocks) {
    for (const t of b.txs) out.set(t.hash, { stage: 'committed', block: b.hash, number: b.number });
  }
  return out;
}

const poolComplete = (p: PipelinePool | null) =>
  !!p && p.pendingTotal === p.pending.length && p.proposedTotal === p.proposed.length;

const proposalKey = (shortId: string, uncleIndex: number | null) =>
  `${shortId}|${uncleIndex ?? ''}`;

function proposalStates(b: PipelineBlock): Map<string, PipelineProposalState> {
  return new Map(b.proposals.map((p) => [proposalKey(p.shortId, p.uncleIndex), p.state]));
}

export function diffSnapshots(prev: PipelineSnapshot | null, next: PipelineSnapshot): Transition {
  if (!prev) return { mode: 'boot', events: [], contact: 0, settleAt: 0 };

  const prevByHash = new Map(prev.blocks.map((b) => [b.hash, b]));
  const nextByHash = new Map(next.blocks.map((b) => [b.hash, b]));
  const oldest = next.blocks.length ? next.blocks[next.blocks.length - 1].number : Infinity;
  const cardEvents: TransitionEvent[] = [];
  const retired: PipelineBlock[] = [];
  const orphaned: PipelineBlock[] = [];
  for (const b of prev.blocks) {
    if (nextByHash.has(b.hash)) continue;
    if (b.number < oldest) retired.push(b);
    else orphaned.push(b);
  }
  const entered = next.blocks.filter((b) => !prevByHash.has(b.hash));

  const advance = next.tip.number - prev.tip.number;
  const newest = next.blocks[0];
  const mode: TransitionMode =
    orphaned.length > 0
      ? 'settle'
      : advance === 1 && newest !== undefined && newest.parentHash === prev.tip.hash
        ? 'mined'
        : advance === 0
          ? 'update'
          : 'settle';

  if (mode === 'settle') {
    const events: TransitionEvent[] = [{ kind: 'settle' }];
    for (const b of orphaned)
      events.push({ kind: 'orphan', block: b.hash, number: b.number, at: 0 });
    for (const b of retired)
      events.push({ kind: 'retire', block: b.hash, number: b.number, at: 0 });
    for (const b of entered) events.push({ kind: 'enter', block: b.hash, number: b.number, at: 0 });
    return { mode, events, contact: 0, settleAt: 0 };
  }

  const before = places(prev);
  const after = places(next);
  const newBlocks = new Set(entered.map((b) => b.hash));
  const commits: TransitionEvent[] = [];
  const reveals: string[] = [];
  const rankInBlock = new Map<string, number>();
  for (const b of next.blocks) {
    if (!newBlocks.has(b.hash)) continue;
    for (const t of b.txs) {
      const was = before.get(t.hash);
      if (was && was.stage !== 'committed') {
        const rank = rankInBlock.get(b.hash) ?? 0;
        rankInBlock.set(b.hash, rank + 1);
        commits.push({
          kind: 'commit',
          hash: t.hash,
          block: b.hash,
          number: b.number,
          rank,
          at: 0,
        });
      } else if (!was) {
        reveals.push(t.hash);
      }
    }
  }

  const proposes: TransitionEvent[] = [];
  const returns: string[] = [];
  const arrivals: string[] = [];
  const prevComplete = poolComplete(prev.pool);
  for (const t of [...(next.pool?.pending ?? []), ...(next.pool?.proposed ?? [])]) {
    const was = before.get(t.hash);
    const now = after.get(t.hash) as TxPlace;
    if (!was) arrivals.push(t.hash);
    else if (was.stage === 'pending' && now.stage === 'proposed')
      proposes.push({ kind: 'propose', hash: t.hash, rank: proposes.length, at: 0 });
    else if (was.stage === 'proposed' && now.stage === 'pending') returns.push(t.hash);
  }

  const contact = mode === 'mined' ? contactTime(commits.length, proposes.length) : 0;
  const settleAt = mode === 'mined' ? contact + SETTLE_AFTER_CONTACT : 0;

  const events: TransitionEvent[] = [...commits, ...proposes];
  for (const hash of arrivals) {
    events.push({ kind: prevComplete ? 'arrive' : 'reveal', hash, at: 0 });
  }
  for (const hash of reveals) events.push({ kind: 'reveal', hash, at: contact });
  returns.forEach((hash, rank) => events.push({ kind: 'return', hash, rank, at: settleAt }));

  // A pool tx that vanished is a drop only when every list it could be hiding in is complete.
  const provable =
    poolComplete(next.pool) &&
    next.blocks.every((b) => !newBlocks.has(b.hash) || b.omittedTxs === 0);
  for (const t of [...(prev.pool?.pending ?? []), ...(prev.pool?.proposed ?? [])]) {
    if (after.has(t.hash)) continue;
    events.push({ kind: provable ? 'drop' : 'hide', hash: t.hash, at: settleAt });
  }

  for (const b of next.blocks) {
    const old = prevByHash.get(b.hash);
    if (!old) continue;
    const was = proposalStates(old);
    for (const p of b.proposals) {
      const from = was.get(proposalKey(p.shortId, p.uncleIndex));
      if (from === undefined) {
        throw new Error(
          `pipeline: block #${b.number} ${b.hash} gained proposal ${p.shortId} between snapshots`
        );
      }
      if (from !== p.state) {
        events.push({
          kind: 'resolve',
          block: b.hash,
          shortId: p.shortId,
          uncleIndex: p.uncleIndex,
          from,
          to: p.state,
          at: contact,
        });
      }
    }
  }

  for (const b of retired)
    cardEvents.push({ kind: 'retire', block: b.hash, number: b.number, at: settleAt });
  events.push(...cardEvents);

  if (mode === 'mined') {
    const pays = newest.cellbase.pays;
    const payeeBlock = pays ? nextByHash.get(pays.hash) : undefined;
    if (pays && payeeBlock && payeeBlock.number !== pays.number) {
      throw new Error(
        `pipeline: cellbase of #${newest.number} pays ${pays.hash} as #${pays.number}, but that block is #${payeeBlock.number}`
      );
    }
    events.push({
      kind: 'mined',
      block: newest.hash,
      number: newest.number,
      cellbase: newest.cellbase.hash,
      contact,
      payee: pays && payeeBlock ? { ...pays, cellbase: payeeBlock.cellbase.hash } : null,
    });
  }

  return { mode, events, contact, settleAt };
}
