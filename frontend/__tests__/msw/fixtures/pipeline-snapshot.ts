import type {
  PipelineBlock,
  PipelineCellbasePays,
  PipelineCellbaseReward,
  PipelinePoolTx,
  PipelineProposal,
  PipelineSnapshot,
  PipelineTx,
} from '@/lib/api';
import { BLOCK_FIELD_CAPACITY } from '@/lib/pipeline/layout';

/**
 * A `GET /pipeline` snapshot that is internally consistent under mainnet consensus
 * (closest 2, farthest 10): every committed tx has its proposal in a visible-or-older block of
 * its window, every pool-proposed tx has a waiting/early proposal, and `next` only predicts
 * committable txs. It covers an uncle with proposals, all four proposal states, pending + proposed
 * pool txs, a block with `omittedTxs > 0` and a tx whose cycles are still pending.
 */

export const FIXTURE_TIP = 18_734_172;
export const FIXTURE_NOW = Date.UTC(2026, 9, 23, 8, 0, 0);
const T = FIXTURE_TIP;
const BLOCKS = 24;
const CELLBASE_SIZE = 336;
/**
 * The backend lists at most this many txs per block (crates/api/src/routes/pipeline.rs): a block
 * field less the cell its cellbase takes. The rest are counted as `omittedTxs`.
 */
const LISTED_TXS_PER_BLOCK = BLOCK_FIELD_CAPACITY - 1;

// The counter sits in the first 10 bytes so proposal short ids never collide.
const hex64 = (prefix: string, n: number) =>
  `0x${(prefix + n.toString(16).padStart(12, '0')).padEnd(64, 'a')}`;
export const blockHash = (n: number) => hex64('b1', n);
export const uncleHash = (n: number) => hex64('0c', n);
export const cellbaseHash = (n: number) => hex64('cb', n);
export const committedTxHash = (block: number, i: number) => hex64('7c', block * 1000 + i);
export const poolTxHash = (name: string) =>
  hex64('9e', [...name].reduce((acc, ch) => acc * 131 + ch.charCodeAt(0), 7) % 2 ** 40);
export const shortIdOf = (hash: string) => hash.slice(0, 22);

/** Blocks up to this height pay no reward: their target would be genesis or earlier. */
const FINALIZATION_DELAY = 11;

/**
 * The reward block `p` earned, paid by the cellbase of block `p + 11`, in exact shannons: an
 * epoch's primary issuance, a secondary share that moves with the block and a fee split. `total`
 * is the exact sum of the parts, the capacity the backend checks against the cellbase output.
 */
export function rewardOf(p: number): PipelineCellbaseReward {
  const primary = BigInt(53_272_450_532);
  const secondary = BigInt(10_228_311_644 + (p % 97) * 1_301);
  const committed = BigInt(1_000_000 + (p % 13) * 77_777);
  const proposal = BigInt((p % 7) * 12_345);
  return {
    primary: String(primary),
    secondary: String(secondary),
    committed: String(committed),
    proposal: String(proposal),
    total: String(primary + secondary + committed + proposal),
    paid: true,
  };
}

/** What the cellbase of block `n` pays: block `n − 11` and its reward; null up to block 11. */
export function paysOf(n: number): PipelineCellbasePays | null {
  const p = n - FINALIZATION_DELAY;
  return p > 0 ? { number: p, hash: blockHash(p), reward: rewardOf(p) } : null;
}

const sizeFor = (seed: number) => 330 + ((seed * 137) % 900);
const feeFor = (size: number, rateKb: number) => String(Math.ceil((size * rateKb) / 1000));

function emptyBlock(n: number): PipelineBlock {
  return {
    number: n,
    hash: blockHash(n),
    parentHash: blockHash(n - 1),
    timestamp: new Date(FIXTURE_NOW - (T - n) * 8000).toISOString(),
    txCount: 1,
    bytes: CELLBASE_SIZE,
    cycles: 0,
    cyclesPending: false,
    proposals: [],
    uncles: [],
    txs: [],
    omittedTxs: 0,
    cellbase: {
      hash: cellbaseHash(n),
      size: CELLBASE_SIZE,
      pays: paysOf(n),
    },
  };
}

interface CommitOptions {
  size?: number;
  cycles?: number | null;
  cyclesStatus?: PipelineTx['cyclesStatus'];
  uncleIndex?: number | null;
  rateKb?: number;
}

export function buildPipelineSnapshot(): PipelineSnapshot {
  const blocks = new Map<number, PipelineBlock>();
  for (let n = T - BLOCKS + 1; n <= T; n++) blocks.set(n, emptyBlock(n));
  const at = (n: number) => {
    const b = blocks.get(n);
    if (!b) throw new Error(`fixture: block ${n} is not visible`);
    return b;
  };
  const allTxs = new Map<number, PipelineTx[]>();

  const propose = (p: number, proposal: PipelineProposal) => {
    const b = blocks.get(p);
    if (b) b.proposals.push(proposal);
  };

  // A tx committed in block c, proposed in block p (which must lie in [c − 10, c − 2]).
  const commit = (c: number, i: number, p: number, opts: CommitOptions = {}) => {
    if (c - p < 2 || c - p > 10) throw new Error(`fixture: ${p} is outside the window of ${c}`);
    const hash = committedTxHash(c, i);
    const size = opts.size ?? sizeFor(c + i);
    const rateKb = opts.rateKb ?? 1000 + ((c + i) % 5) * 250;
    const cycles =
      opts.cycles === undefined ? 1_500_000 + ((c * 7 + i) % 9) * 900_000 : opts.cycles;
    const tx: PipelineTx = {
      hash,
      size,
      fee: feeFor(size, rateKb),
      feeRate: String(rateKb),
      cycles,
      cyclesStatus: opts.cyclesStatus ?? 'done',
      proposedIn: { block: p, uncleIndex: opts.uncleIndex ?? null },
    };
    const list = allTxs.get(c) ?? [];
    list.push(tx);
    allTxs.set(c, list);
    propose(p, {
      shortId: shortIdOf(hash),
      uncleIndex: opts.uncleIndex ?? null,
      state: 'committed',
      txHash: hash,
      committedBlock: c,
    });
  };

  // Uncle at T−2, carried by T−1.
  at(T - 1).uncles.push({ number: T - 2, hash: uncleHash(T - 2) });

  // Block T: four commits; the first one's cycles are still being computed.
  commit(T, 0, T - 3, { cycles: null, cyclesStatus: 'pending', size: 2400 });
  commit(T, 1, T - 3, { size: 9600, cycles: 60_000_000 });
  commit(T, 2, T - 2);
  commit(T, 3, T - 10, { size: 5400, cycles: 12_000_000 });
  // Block T−1 commits two txs proposed in T−4, one through T−4's uncle-free zone.
  commit(T - 1, 0, T - 4);
  commit(T - 1, 1, T - 4);
  commit(T - 2, 0, T - 5);
  // Block T−5 is full: 299 commits, all proposed in T−7 → LISTED_TXS_PER_BLOCK listed, the rest
  // omitted.
  for (let i = 0; i < 299; i++) commit(T - 5, i, T - 7, { size: 330 + (i % 7) * 40 });
  // A commit of the uncle-carried proposal path: T−6 carried an uncle whose proposal T−4 commits.
  at(T - 6).uncles.push({ number: T - 7, hash: uncleHash(T - 7) });
  commit(T - 4, 0, T - 6, { uncleIndex: 0 });
  commit(T - 10, 0, T - 12);
  for (let n = T - BLOCKS + 3; n <= T - 11; n++) {
    if (n === T - 12 || n === T - 10) continue;
    for (let i = 0; i < (n % 3) + 1; i++) commit(n, i, n - 2);
  }

  // Pool. Pending txs carry no proposal; proposed txs carry the newest zone hit.
  const poolTx = (
    name: string,
    k: number,
    proposedIn: PipelinePoolTx['proposedIn'],
    size = sizeFor(k + 3)
  ): PipelinePoolTx => {
    const rateKb = 1000 + (k % 4) * 500;
    return {
      hash: poolTxHash(name),
      size,
      fee: feeFor(size, rateKb),
      feeRate: String(rateKb),
      cycles: 1_200_000 + k * 800_000,
      arrivedAt: FIXTURE_NOW - (20 - k) * 1500,
      proposedIn,
    };
  };
  const pending = ['q0', 'q1', 'q2', 'q3', 'q4'].map((n, k) => poolTx(n, k, null));
  const early = ['e0', 'e1', 'e2'].map((n, k) => poolTx(n, 5 + k, { block: T, uncleIndex: null }));
  const viaUncle = ['u0', 'u1'].map((n, k) => poolTx(n, 8 + k, { block: T - 1, uncleIndex: 0 }));
  const ownWaiting = ['w0', 'w1'].map((n, k) =>
    poolTx(n, 10 + k, { block: T - 1, uncleIndex: null }, k === 0 ? 1600 : undefined)
  );
  const older = [
    poolTx('w2', 12, { block: T - 3, uncleIndex: null }),
    poolTx('w3', 13, { block: T - 9, uncleIndex: null }),
  ];
  const proposed = [...early, ...ownWaiting, ...viaUncle, ...older];
  for (const tx of proposed) {
    const p = tx.proposedIn as NonNullable<PipelinePoolTx['proposedIn']>;
    propose(p.block, {
      shortId: shortIdOf(tx.hash),
      uncleIndex: p.uncleIndex,
      state: p.block === T ? 'early' : 'waiting',
      txHash: tx.hash,
      committedBlock: null,
    });
  }
  // Expired proposals: one whose tx left the pool, one whose tx returned to pending (q4).
  propose(T - 12, {
    shortId: '0x0dead00000000000000a1',
    uncleIndex: null,
    state: 'expired',
    txHash: null,
    committedBlock: null,
  });
  propose(T - 12, {
    shortId: shortIdOf(pending[4].hash),
    uncleIndex: null,
    state: 'expired',
    txHash: pending[4].hash,
    committedBlock: null,
  });

  for (const [n, b] of blocks) {
    const txs = (allTxs.get(n) ?? []).slice().sort((a, b2) => b2.size - a.size);
    b.txCount = 1 + txs.length;
    b.bytes = CELLBASE_SIZE + txs.reduce((s, t) => s + t.size, 0);
    const pendingCycles = txs.some((t) => t.cycles === null);
    b.cycles = pendingCycles ? null : txs.reduce((s, t) => s + (t.cycles as number), 0);
    b.cyclesPending = pendingCycles;
    b.txs = txs.slice(0, LISTED_TXS_PER_BLOCK);
    b.omittedTxs = txs.length - b.txs.length;
    // Zone order: the block's own proposals first, then each uncle's.
    b.proposals.sort((a, c) => (a.uncleIndex ?? -1) - (c.uncleIndex ?? -1));
  }

  const committable = proposed
    .filter((t) => {
      const p = (t.proposedIn as NonNullable<PipelinePoolTx['proposedIn']>).block;
      return p >= T - 9 && p <= T - 1;
    })
    .sort((a, b) => Number(b.feeRate) - Number(a.feeRate) || a.arrivedAt - b.arrivedAt);

  return {
    status: 'ok',
    consensus: {
      proposalWindowClosest: 2,
      proposalWindowFarthest: 10,
      maxBlockBytes: 597_000,
      maxBlockCycles: 3_500_000_000,
      maxBlockProposals: 1500,
    },
    tip: { number: T, hash: blockHash(T) },
    poolTip: { number: T, hash: blockHash(T) },
    blocks: [...blocks.values()].sort((a, b) => b.number - a.number),
    pool: {
      pending,
      proposed,
      pendingTotal: pending.length,
      proposedTotal: proposed.length,
    },
    next: {
      // Listed like a block's txs: by size, descending.
      txs: committable
        .slice()
        .sort((a, b) => b.size - a.size)
        .map((t) => ({ hash: t.hash, size: t.size })),
      omittedTxs: 0,
      bytes: committable.reduce((s, t) => s + t.size, 0),
      cycles: committable.reduce((s, t) => s + t.cycles, 0),
      proposals: Math.min(pending.length, 1500),
    },
  };
}

export const pipelineSnapshot: PipelineSnapshot = buildPipelineSnapshot();

interface MineOptions {
  /** Pool-proposed txs the new block commits (each must be inside its window). */
  commit: string[];
  /** Pending txs the new block proposes. */
  propose: string[];
}

/**
 * The snapshot after one more block on top of `s`, applying the same rules the backend uses:
 * commits resolve their proposals, the old tip's early proposals start waiting, proposals whose
 * window closed expire (their pool txs leave the pool), and `next` is re-predicted.
 */
export function mineNext(s: PipelineSnapshot, o: MineOptions): PipelineSnapshot {
  const pool = s.pool;
  if (!pool || !s.next) throw new Error('fixture: mineNext needs a pool');
  const n = s.tip.number + 1;
  const far = s.consensus.proposalWindowFarthest;
  const close = s.consensus.proposalWindowClosest;
  const blocks = s.blocks.map((b) => ({ ...b, proposals: b.proposals.map((p) => ({ ...p })) }));
  const byNumber = new Map(blocks.map((b) => [b.number, b]));
  const proposed = new Map(pool.proposed.map((t) => [t.hash, t]));
  const pending = new Map(pool.pending.map((t) => [t.hash, t]));

  const txs: PipelineTx[] = o.commit.map((hash) => {
    const t = proposed.get(hash);
    if (!t?.proposedIn) throw new Error(`fixture: ${hash} is not proposed`);
    const p = t.proposedIn.block;
    if (n - p < close || n - p > far) throw new Error(`fixture: ${hash} cannot commit in #${n}`);
    const proposal = byNumber
      .get(p)
      ?.proposals.find((q) => q.txHash === hash && q.uncleIndex === t.proposedIn?.uncleIndex);
    if (!proposal) throw new Error(`fixture: no proposal for ${hash} in #${p}`);
    proposal.state = 'committed';
    proposal.committedBlock = n;
    proposed.delete(hash);
    return {
      hash,
      size: t.size,
      fee: t.fee,
      feeRate: t.feeRate,
      cycles: t.cycles,
      cyclesStatus: 'done',
      proposedIn: t.proposedIn,
    };
  });
  for (const b of blocks) {
    for (const p of b.proposals) {
      if (p.state === 'early') p.state = 'waiting';
      if (p.state === 'waiting' && b.number + far <= n) {
        p.state = 'expired';
        if (p.txHash) proposed.delete(p.txHash);
      }
    }
  }
  const own: PipelineProposal[] = o.propose.map((hash) => {
    const t = pending.get(hash);
    if (!t) throw new Error(`fixture: ${hash} is not pending`);
    pending.delete(hash);
    proposed.set(hash, { ...t, proposedIn: { block: n, uncleIndex: null } });
    return {
      shortId: shortIdOf(hash),
      uncleIndex: null,
      state: 'early',
      txHash: hash,
      committedBlock: null,
    };
  });
  const sorted = txs.slice().sort((a, b) => b.size - a.size);
  const mined: PipelineBlock = {
    number: n,
    hash: blockHash(n),
    parentHash: s.tip.hash,
    timestamp: new Date(FIXTURE_NOW + (n - T) * 8000).toISOString(),
    txCount: 1 + sorted.length,
    bytes: CELLBASE_SIZE + sorted.reduce((sum, t) => sum + t.size, 0),
    cycles: sorted.reduce((sum, t) => sum + (t.cycles as number), 0),
    cyclesPending: false,
    proposals: own,
    uncles: [],
    txs: sorted,
    omittedTxs: 0,
    cellbase: {
      hash: cellbaseHash(n),
      size: CELLBASE_SIZE,
      pays: paysOf(n),
    },
  };
  const nextBlocks = [mined, ...blocks.slice(0, blocks.length - 1)];
  const pendingList = [...pending.values()];
  const proposedList = [...proposed.values()];
  const committable = proposedList
    .filter((t) => {
      const p = (t.proposedIn as NonNullable<PipelinePoolTx['proposedIn']>).block;
      return p >= n - (far - 1) && p <= n - (close - 1);
    })
    .sort((a, b) => Number(b.feeRate) - Number(a.feeRate) || a.arrivedAt - b.arrivedAt);
  return {
    ...s,
    tip: { number: n, hash: mined.hash },
    poolTip: { number: n, hash: mined.hash },
    blocks: nextBlocks,
    pool: {
      pending: pendingList,
      proposed: proposedList,
      pendingTotal: pendingList.length,
      proposedTotal: proposedList.length,
    },
    next: {
      // Listed like a block's txs: by size, descending.
      txs: committable
        .slice()
        .sort((a, b) => b.size - a.size)
        .map((t) => ({ hash: t.hash, size: t.size })),
      omittedTxs: 0,
      bytes: committable.reduce((sum, t) => sum + t.size, 0),
      cycles: committable.reduce((sum, t) => sum + t.cycles, 0),
      proposals: Math.min(pendingList.length, s.consensus.maxBlockProposals),
    },
  };
}
