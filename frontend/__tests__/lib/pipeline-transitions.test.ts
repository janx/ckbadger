import { describe, expect, it } from 'vitest';
import type {
  PipelineBlock,
  PipelinePoolTx,
  PipelineProposal,
  PipelineProposalState,
  PipelineSnapshot,
} from '@/lib/api';
import {
  SETTLE_AFTER_CONTACT,
  contactTime,
  diffSnapshots,
  type TransitionEvent,
} from '@/lib/pipeline/transitions';
import { rewardOf } from '@/__tests__/msw/fixtures/pipeline-snapshot';

const bh = (n: number, fork = '') => `0xb${fork}${n}`;

interface BlkOpts {
  hash?: string;
  parent?: string;
  txs?: string[];
  proposals?: [string, PipelineProposalState][];
  omitted?: number;
}

function blk(n: number, o: BlkOpts = {}): PipelineBlock {
  const txs = o.txs ?? [];
  return {
    number: n,
    hash: o.hash ?? bh(n),
    parentHash: o.parent ?? bh(n - 1),
    timestamp: '2026-10-23T08:00:00Z',
    txCount: 1 + txs.length + (o.omitted ?? 0),
    bytes: 336,
    cycles: 0,
    cyclesPending: false,
    proposals: (o.proposals ?? []).map(
      ([shortId, state]): PipelineProposal => ({
        shortId,
        uncleIndex: null,
        state,
        txHash: null,
        committedBlock: null,
      })
    ),
    uncles: [],
    txs: txs.map((hash) => ({
      hash,
      size: 500,
      fee: '500',
      feeRate: '1000',
      cycles: 2_000_000,
      cyclesStatus: 'done' as const,
      proposedIn: { block: n - 3, uncleIndex: null },
    })),
    omittedTxs: o.omitted ?? 0,
    cellbase: {
      hash: `${o.hash ?? bh(n)}-cb`,
      size: 336,
      pays: { number: n - 11, hash: bh(n - 11), reward: rewardOf(n - 11) },
    },
  };
}

function ptx(hash: string, proposedAt: number | null = null): PipelinePoolTx {
  return {
    hash,
    size: 500,
    fee: '500',
    feeRate: '1000',
    cycles: 2_000_000,
    arrivedAt: 1,
    proposedIn: proposedAt === null ? null : { block: proposedAt, uncleIndex: null },
  };
}

interface SnapOpts {
  pending?: string[];
  proposed?: [string, number][];
  pendingTotal?: number;
  proposedTotal?: number;
}

function snap(blocks: PipelineBlock[], o: SnapOpts = {}): PipelineSnapshot {
  const pending = (o.pending ?? []).map((h) => ptx(h));
  const proposed = (o.proposed ?? []).map(([h, p]) => ptx(h, p));
  return {
    status: 'ok',
    consensus: {
      proposalWindowClosest: 2,
      proposalWindowFarthest: 10,
      maxBlockBytes: 597_000,
      maxBlockCycles: 3_500_000_000,
      maxBlockProposals: 1500,
    },
    tip: { number: blocks[0].number, hash: blocks[0].hash },
    poolTip: { number: blocks[0].number, hash: blocks[0].hash },
    blocks,
    pool: {
      pending,
      proposed,
      pendingTotal: o.pendingTotal ?? pending.length,
      proposedTotal: o.proposedTotal ?? proposed.length,
    },
    next: { txs: [], omittedTxs: 0, bytes: 0, cycles: 0, proposals: pending.length },
  };
}

/** Blocks top … top − count + 1, newest first. */
const chain = (top: number, count = 24, extra: Record<number, BlkOpts> = {}) =>
  Array.from({ length: count }, (_, i) => blk(top - i, extra[top - i]));

const kinds = (events: TransitionEvent[]) => events.map((e) => e.kind);
const ofKind = <K extends TransitionEvent['kind']>(events: TransitionEvent[], kind: K) =>
  events.filter((e): e is Extract<TransitionEvent, { kind: K }> => e.kind === kind);

describe('diffSnapshots', () => {
  it('boots on the first snapshot without any animation event', () => {
    const t = diffSnapshots(null, snap(chain(100), { pending: ['a'] }));
    expect(t.mode).toBe('boot');
    expect(t.events).toEqual([]);
  });

  it('reports a new pool tx as an arrival', () => {
    const t = diffSnapshots(
      snap(chain(100), { pending: ['a'] }),
      snap(chain(100), { pending: ['a', 'b'] })
    );
    expect(t.mode).toBe('update');
    expect(t.events).toEqual([{ kind: 'arrive', hash: 'b', at: 0 }]);
  });

  it('does not claim an arrival when the previous pool list was truncated', () => {
    const t = diffSnapshots(
      snap(chain(100), { pending: ['a'], pendingTotal: 400 }),
      snap(chain(100), { pending: ['a', 'b'], pendingTotal: 400 })
    );
    expect(t.events).toEqual([{ kind: 'reveal', hash: 'b', at: 0 }]);
  });

  it('turns pending → proposed into a ranked proposal when the next block proposes it', () => {
    const t = diffSnapshots(
      snap(chain(100), { pending: ['a', 'b'] }),
      snap(
        chain(101, 24, {
          101: {
            proposals: [
              ['a', 'early'],
              ['b', 'early'],
            ],
          },
        }),
        {
          proposed: [
            ['a', 101],
            ['b', 101],
          ],
        }
      )
    );
    expect(t.mode).toBe('mined');
    expect(ofKind(t.events, 'propose')).toEqual([
      { kind: 'propose', hash: 'a', rank: 0, at: 0 },
      { kind: 'propose', hash: 'b', rank: 1, at: 0 },
    ]);
  });

  it('commits pool txs into the new block with a per-block stagger rank', () => {
    const t = diffSnapshots(
      snap(chain(100), {
        proposed: [
          ['x', 97],
          ['y', 98],
          ['z', 99],
        ],
      }),
      snap(chain(101, 24, { 101: { txs: ['y', 'x'] } }), { proposed: [['z', 99]] })
    );
    expect(ofKind(t.events, 'commit')).toEqual([
      { kind: 'commit', hash: 'y', block: bh(101), number: 101, rank: 0, at: 0 },
      { kind: 'commit', hash: 'x', block: bh(101), number: 101, rank: 1, at: 0 },
    ]);
  });

  it('reveals at contact a committed tx that was never seen in the pool', () => {
    const t = diffSnapshots(
      snap(chain(100), { proposed: [['x', 97]] }),
      snap(chain(101, 24, { 101: { txs: ['x', 'unseen'] } }))
    );
    expect(ofKind(t.events, 'reveal')).toEqual([{ kind: 'reveal', hash: 'unseen', at: t.contact }]);
  });

  it('returns a proposed tx to pending at settle time', () => {
    const t = diffSnapshots(
      snap(chain(100), { proposed: [['x', 90]] }),
      snap(chain(101), { pending: ['x'] })
    );
    expect(ofKind(t.events, 'return')).toEqual([
      { kind: 'return', hash: 'x', rank: 0, at: t.settleAt },
    ]);
  });

  it('drops a tx that left the pool without being committed', () => {
    const t = diffSnapshots(snap(chain(100), { pending: ['a'] }), snap(chain(100)));
    expect(t.events).toEqual([{ kind: 'drop', hash: 'a', at: 0 }]);
  });

  it('only fades a vanished tx when the new lists cannot prove it was dropped', () => {
    const truncated = diffSnapshots(
      snap(chain(100), { pending: ['a', 'b'] }),
      snap(chain(100), { pending: ['b'], pendingTotal: 300 })
    );
    expect(truncated.events).toEqual([{ kind: 'hide', hash: 'a', at: 0 }]);
    const omitted = diffSnapshots(
      snap(chain(100), { proposed: [['x', 95]] }),
      snap(chain(101, 24, { 101: { omitted: 3 } }))
    );
    expect(ofKind(omitted.events, 'hide')).toEqual([
      { kind: 'hide', hash: 'x', at: omitted.settleAt },
    ]);
    expect(ofKind(omitted.events, 'drop')).toEqual([]);
  });

  it('retires the block that slides out of the visible range', () => {
    const t = diffSnapshots(snap(chain(100)), snap(chain(101)));
    expect(ofKind(t.events, 'retire')).toEqual([
      { kind: 'retire', block: bh(77), number: 77, at: t.settleAt },
    ]);
  });

  it('reports tip + 1 as a mined block with its contact time and payee', () => {
    const t = diffSnapshots(
      snap(chain(100), {
        pending: ['a'],
        proposed: [
          ['x', 97],
          ['y', 98],
        ],
      }),
      snap(chain(101, 24, { 101: { txs: ['x', 'y'], proposals: [['a', 'early']] } }), {
        proposed: [['a', 101]],
      })
    );
    const contact = contactTime(2, 1);
    expect(contact).toBe(Math.max(40 + 16 + 720, 120 + 0 + 560));
    expect(t.contact).toBe(contact);
    // The landing beat: 560 ms at the NEXT position after contact before the block moves on.
    expect(SETTLE_AFTER_CONTACT).toBe(560);
    expect(t.settleAt).toBe(contact + SETTLE_AFTER_CONTACT);
    expect(ofKind(t.events, 'mined')).toEqual([
      {
        kind: 'mined',
        block: bh(101),
        number: 101,
        cellbase: `${bh(101)}-cb`,
        contact,
        payee: { number: 90, hash: bh(90), cellbase: `${bh(90)}-cb` },
      },
    ]);
  });

  it('has no payee when the paid block is not on screen', () => {
    const t = diffSnapshots(snap(chain(100, 5)), snap(chain(101, 5)));
    expect(ofKind(t.events, 'mined')[0].payee).toBeNull();
  });

  it('only settles when several blocks arrive at once (catching up)', () => {
    const t = diffSnapshots(
      snap(chain(100), { pending: ['a'], proposed: [['x', 97]] }),
      snap(chain(103, 24, { 103: { txs: ['x'] } }), { proposed: [['a', 102]] })
    );
    expect(t.mode).toBe('settle');
    expect(kinds(t.events)).toContain('settle');
    for (const k of ['mined', 'commit', 'propose', 'resolve', 'arrive'] as const) {
      expect(ofKind(t.events, k)).toEqual([]);
    }
    expect(ofKind(t.events, 'retire').map((e) => e.number)).toEqual([79, 78, 77]);
  });

  it('orphans a card whose height got a new hash and enters its replacement', () => {
    const before = chain(100);
    const after = [blk(100, { hash: bh(100, 'f') }), ...chain(99, 23)];
    const t = diffSnapshots(snap(before), snap(after));
    expect(t.mode).toBe('settle');
    expect(ofKind(t.events, 'orphan')).toEqual([
      { kind: 'orphan', block: bh(100), number: 100, at: 0 },
    ]);
    expect(ofKind(t.events, 'enter')).toEqual([
      { kind: 'enter', block: bh(100, 'f'), number: 100, at: 0 },
    ]);
    expect(ofKind(t.events, 'mined')).toEqual([]);
  });

  it('treats a new tip whose parent is not the old tip as a reorg', () => {
    const after = [
      blk(101, { parent: bh(100, 'f') }),
      blk(100, { hash: bh(100, 'f') }),
      ...chain(99, 22),
    ];
    const t = diffSnapshots(snap(chain(100)), snap(after));
    expect(t.mode).toBe('settle');
    expect(ofKind(t.events, 'orphan').map((e) => e.block)).toEqual([bh(100)]);
  });

  it('fails fast when a known block hash gains a proposal between snapshots', () => {
    expect(() =>
      diffSnapshots(
        snap(chain(100, 24, { 97: { proposals: [['p1', 'waiting']] } })),
        snap(
          chain(100, 24, {
            97: {
              proposals: [
                ['p1', 'waiting'],
                ['p9', 'waiting'],
              ],
            },
          })
        )
      )
    ).toThrow(/#97 .* gained proposal p9/);
  });

  it('resolves proposal chips that changed state at the contact time', () => {
    const t = diffSnapshots(
      snap(
        chain(100, 24, {
          97: {
            proposals: [
              ['p1', 'waiting'],
              ['p2', 'waiting'],
            ],
          },
        })
      ),
      snap(
        chain(101, 24, {
          97: {
            proposals: [
              ['p1', 'committed'],
              ['p2', 'waiting'],
            ],
          },
        })
      )
    );
    expect(ofKind(t.events, 'resolve')).toEqual([
      {
        kind: 'resolve',
        block: bh(97),
        shortId: 'p1',
        uncleIndex: null,
        from: 'waiting',
        to: 'committed',
        at: t.contact,
      },
    ]);
  });
});
