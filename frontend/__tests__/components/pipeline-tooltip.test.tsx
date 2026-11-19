import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen, within } from '@testing-library/react';
import { T0, tick } from '@/__tests__/utils/still-clock';
import {
  FIXTURE_TIP,
  buildPipelineSnapshot,
  committedTxHash,
  pipelineSnapshot,
  poolTxHash,
} from '@/__tests__/msw/fixtures/pipeline-snapshot';
import type { PipelineBlock, PipelineSnapshot } from '@/lib/api';
import {
  PipelineTooltip,
  pct,
  blockTip,
  cellbaseTip,
  chipTip,
  nextTip,
  predictedFacts,
  predictedTip,
  txTip,
  uncleTip,
  windowTip,
  withOpen,
  type TipContent,
} from '@/components/pipeline/pipeline-tooltip';

const T = FIXTURE_TIP;

// CH-14 and the review's item I: the tooltip's share of a block's room is the one percentage, two
// decimals (it wrote one from 1 %, two below), never a false 0.00% or 100.00%
describe('pct: the share of a block’s room through percentOf', () => {
  it('writes two decimals and bounds the two false results', () => {
    expect(pct(1, 3)).toBe('33.33%');
    expect(pct(25, 1000)).toBe('2.50%');
    expect(pct(5, 1000)).toBe('0.50%');
    expect(pct(0, 1000)).toBe('0.00%');
    expect(pct(1, 1_000_000)).toBe('<0.01%');
    // re-pinned for lane exactness, part B2 (one percentage, cut): 999,999 of 1,000,000 is 99.9999%, 99.99%, which is true of it (half up it read 100.00, bounded >99.99%)
    expect(pct(999_999, 1_000_000)).toBe('99.99%');
    expect(pct(1000, 1000)).toBe('100.00%');
  });
});
const snapshot = buildPipelineSnapshot();

function renderTip(content: TipContent) {
  render(<PipelineTooltip content={content} />);
  return screen.getByTestId('pipeline-tooltip');
}

const classesOf = (el: Element) => el.className.split(/\s+/);

describe('PipelineTooltip', () => {
  it('keeps fee shannons beyond the safe Number range exact', () => {
    const pool = snapshot.pool as NonNullable<PipelineSnapshot['pool']>;
    const tx = pool.pending[0];
    const tip = txTip({ kind: 'pending', tx: { ...tx, fee: '9007199254740993' } }, snapshot);
    expect(tip.rows).toContainEqual(['fee', '9,007,199,254,740,993 sh']);
  });
  // jsdom does not lay out, so the contract is in the classes: values never wrap, and the tooltip
  // takes the width its rows need instead of squeezing the value column.
  it('keeps the proposal window values on one line', () => {
    const tip = renderTip(windowTip(T + 1, snapshot));
    const range = within(tip).getByText(
      `…${String(T - 9).slice(-4)} – …${String(T - 1).slice(-4)}`
    );
    expect(range.tagName).toBe('DD');
    expect(range).toHaveClass('whitespace-nowrap');
    for (const dd of tip.querySelectorAll('dd')) expect(dd).toHaveClass('whitespace-nowrap');
  });

  it('keeps a long value on one line next to its label, the widest row setting the width', () => {
    const pool = snapshot.pool as NonNullable<PipelineSnapshot['pool']>;
    const early = pool.proposed.find((t) => t.hash === poolTxHash('e0'));
    if (!early) throw new Error('fixture: e0 is not proposed');
    const tip = renderTip(txTip({ kind: 'proposed', tx: early, tooEarly: true }, snapshot));
    expect(within(tip).getByText('not yet: proposed in the last block')).toHaveClass(
      'whitespace-nowrap'
    );
    const classes = classesOf(tip);
    // Sized by its content, never narrower than the 268 px it had, never wider than the viewport.
    expect(classes).toContain('w-max');
    expect(classes).toContain('min-w-[268px]');
    expect(classes).toContain('max-w-[calc(100vw-16px)]');
    expect(classes.some((c) => /^w-\[/.test(c))).toBe(false);
  });

  it('wraps the footnote to the width the rows set instead of widening the tooltip', () => {
    const tip = renderTip(windowTip(T + 1, snapshot));
    const foot = within(tip).getByText(/A tx can be committed 2 to 10 blocks/);
    expect(foot).toHaveClass('w-0');
    expect(foot).toHaveClass('min-w-full');
    expect(foot).not.toHaveClass('whitespace-nowrap');
  });
});

// Lane E4: a pending transaction's wait, a running duration, reads the page's one clock while the
// tooltip is up. The panel keeps one tooltip per hovered transaction (the same key keeps the same
// content), so the wait was written once, at the first pointer move, and stood still for as long
// as the pointer rested on the square. The clock stops with the tooltip.
describe('a pending transaction’s wait (E4)', () => {
  afterEach(() => vi.useRealTimers());

  it('moves with no data change while the tooltip is up, and leaves no timer behind', () => {
    vi.useFakeTimers({ now: T0 });
    const pool = snapshot.pool as NonNullable<PipelineSnapshot['pool']>;
    const tip = renderTip(
      txTip({ kind: 'pending', tx: { ...pool.pending[0], arrivedAt: T0 - 20_000 } }, snapshot)
    );
    const waiting = () => within(tip).getByText('waiting').nextElementSibling!.textContent;
    expect(waiting()).toBe('20.0 s');
    tick(1000);
    expect(waiting()).toBe('21.0 s');
    tick(38_000);
    expect(waiting()).toBe('59.0 s');
    tick(1000);
    expect(waiting()).toBe('60.0 s');
    expect(vi.getTimerCount()).toBe(1);
    cleanup();
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe('cellbase tooltip', () => {
  const blockAt = (s: PipelineSnapshot, n: number): PipelineBlock => {
    const b = s.blocks.find((x) => x.number === n);
    if (!b) throw new Error(`fixture: block ${n} is not listed`);
    return b;
  };

  it('breaks the reward it pays down the way the prototype does, in exact amounts', () => {
    // Block T pays block T − 11 = 18,734,161: 532.72450532 CKB primary + 102.28397510 CKB
    // secondary + 1,311,108 sh committed + 61,725 sh proposal = 63,502,220,875 sh.
    const tip = cellbaseTip(blockAt(snapshot, T), snapshot);
    expect(tip.rows).toEqual([
      ['block', '#18,734,172'],
      ['pays reward of', '#18,734,161 · 635.02220875 CKB'],
      ['· base issuance', '635.00848042 CKB'],
      ['· committed fees', '1,311,108 sh'],
      ['· proposal fees', '61,725 sh'],
      ['own reward', 'paid by a later block'],
    ]);
  });

  it('names the block that pays its own reward when that block is listed', () => {
    const tip = cellbaseTip(blockAt(snapshot, T - 11), snapshot);
    expect(tip.rows.at(-1)).toEqual(['own reward', 'paid by #18,734,172']);
  });

  it('keeps every reward value on one line', () => {
    const el = renderTip(cellbaseTip(blockAt(snapshot, T), snapshot));
    const total = within(el).getByText('#18,734,161 · 635.02220875 CKB');
    expect(total.tagName).toBe('DD');
    const dds = el.querySelectorAll('dd');
    expect(dds).toHaveLength(6);
    for (const dd of dds) expect(dd).toHaveClass('whitespace-nowrap');
  });

  it('shows no breakdown for a block that pays no reward yet', () => {
    const early = { ...blockAt(snapshot, T), number: 11 };
    early.cellbase = { ...early.cellbase, pays: null };
    expect(cellbaseTip(early, snapshot).rows).toEqual([
      ['block', '#11'],
      ['pays reward of', 'none yet'],
      ['own reward', 'paid by a later block'],
    ]);
  });

  it('says so when the reward is too small for its own cell and goes unpaid', () => {
    const b = blockAt(snapshot, T);
    const pays = b.cellbase.pays;
    if (!pays) throw new Error('fixture: block T pays');
    const unpaid: PipelineBlock = {
      ...b,
      cellbase: {
        ...b.cellbase,
        pays: {
          ...pays,
          reward: {
            primary: '5900000000',
            secondary: '99000000',
            committed: '900000',
            proposal: '100000',
            total: '6000000000',
            paid: false,
          },
        },
      },
    };
    const tip = cellbaseTip(unpaid, snapshot);
    expect(tip.rows[1]).toEqual(['pays reward of', '#18,734,161 · 60.00000000 CKB · not paid']);
    expect(tip.rows[2]).toEqual(['· base issuance', '59.99000000 CKB']);
    expect(tip.foot).toMatch(
      /less than the occupied capacity of the cell that would hold it, so this cellbase has no output/
    );
  });

  it('serves a fixture whose every reward is the exact sum of its parts', () => {
    for (const b of pipelineSnapshot.blocks) {
      const pays = b.cellbase.pays;
      if (b.number <= 11) {
        expect(pays).toBeNull();
        continue;
      }
      if (!pays) throw new Error(`fixture: block ${b.number} pays nothing`);
      expect(pays.number).toBe(b.number - 11);
      const r = pays.reward;
      expect(
        BigInt(r.primary) + BigInt(r.secondary) + BigInt(r.committed) + BigInt(r.proposal)
      ).toBe(BigInt(r.total));
      expect(r.paid).toBe(true);
    }
  });
});

// Lane E2 follow-up (ruling R4 covers the muted ink): an uncle's tag, "not on the chain", is words,
// so it takes the dim ink (4.57:1 or more on every ground) as its colour and its frame, never the
// pipeline's muted ink (--pipeline-ink-dim, 3.17–3.76:1 by night, 3.84–4.70:1 by day).
describe('the uncle tooltip', () => {
  it('writes its tag in the dim ink, never the muted', () => {
    const block = snapshot.blocks.find((b) => b.number === T - 1) as PipelineBlock;
    const tip = uncleTip(block, 0);
    expect(tip.tag).toBe('not on the chain');
    expect(tip.tone).toBe('var(--c-text-dim)');
    const tag = within(renderTip(tip)).getByText('not on the chain');
    expect(tag.style.color).toBe('var(--c-text-dim)');
  });
});

// Lane K: a click in the pipeline opens the page of what it hits. A tip whose mark opens a page
// ends with one sentence that says which, written from the panel's own decision of what the click
// opens (withOpen); the builders say nothing about clicking, so no tip can name a page the click
// does not open, and the tip of a mark that opens nothing (NEXT, a well, the window) says nothing.
describe('the sentence that closes a tip whose mark opens a page', () => {
  const pool = snapshot.pool as NonNullable<PipelineSnapshot['pool']>;
  const blockAt = (n: number) => snapshot.blocks.find((b) => b.number === n) as PipelineBlock;
  const w0 = pool.proposed.find((t) => t.hash === poolTxHash('w0'));
  if (!w0) throw new Error('fixture: w0 is not proposed');
  const tipCases: [string, TipContent][] = [
    ['a pending tx', txTip({ kind: 'pending', tx: pool.pending[0] }, snapshot)],
    ['a proposed tx', txTip({ kind: 'proposed', tx: w0, tooEarly: false }, snapshot)],
    [
      'a committed tx',
      txTip(
        {
          kind: 'committed',
          tx: blockAt(T).txs.find(
            (t) => t.hash === committedTxHash(T, 2)
          ) as PipelineBlock['txs'][number],
          block: blockAt(T),
        },
        snapshot
      ),
    ],
    ['a cellbase', txTip({ kind: 'cellbase', block: blockAt(T) }, snapshot)],
    ['a proposal dot', chipTip(blockAt(T - 1), blockAt(T - 1).proposals[0], snapshot)],
    ['a block', blockTip(blockAt(T), snapshot)],
    ['an uncle', uncleTip(blockAt(T - 1), 0)],
    ['a predicted tx', predictedTip({ hash: w0.hash, size: w0.size }, w0, snapshot)],
    [
      'a predicted tx not listed',
      predictedTip({ hash: `0x${'ab'.repeat(32)}`, size: 900 }, null, snapshot),
    ],
    ['NEXT', nextTip(snapshot)],
    ['the window', windowTip(T + 1, snapshot)],
  ];

  it.each(tipCases)('builds the tip of %s without a word about clicking or pinning', (_, tip) => {
    expect(tip.foot ?? '').not.toMatch(/\b(click|tap|pin)/i);
  });

  it('closes a tip with exactly one sentence naming the page, after the foot it had', () => {
    const block = withOpen(blockTip(blockAt(T), snapshot), 'block');
    expect(block.foot).toBe('Click to open the block.');
    const proposed = withOpen(
      txTip({ kind: 'proposed', tx: w0, tooEarly: false }, snapshot),
      'transaction'
    );
    expect(proposed.foot).toBe(
      `The tx waits in the pool; its proposal sits in block #${(T - 1).toLocaleString('en-US')}. Click to open the transaction.`
    );
    const pending = withOpen(
      txTip({ kind: 'pending', tx: pool.pending[0] }, snapshot),
      'transaction'
    );
    expect(pending.foot).toBe('Click to open the transaction.');
    for (const [, tip] of tipCases) {
      for (const page of ['transaction', 'block'] as const) {
        const foot = withOpen(tip, page).foot as string;
        expect(foot.match(/Click to open/g)).toHaveLength(1);
        expect(foot.endsWith(`Click to open the ${page}.`)).toBe(true);
      }
    }
  });

  it('leaves the tip of a mark that opens nothing as it is', () => {
    const tip = windowTip(T + 1, snapshot);
    expect(withOpen(tip, null)).toBe(tip);
  });

  it('renders the closing sentence as the last words of the footnote', () => {
    const el = renderTip(withOpen(uncleTip(blockAt(T - 1), 0), 'block'));
    expect(el.lastElementChild?.textContent).toMatch(/them\. Click to open the block\.$/);
  });
});

// Lane S2: a predicted tx's first facts, which its tip opens with and a pinned predicted tx's
// readout says (the readout of one whose pool tx is not listed), from this one function.
describe('predictedFacts: what a predicted tx is, for its tip and its readout', () => {
  const pool = snapshot.pool as NonNullable<PipelineSnapshot['pool']>;
  const w0 = pool.proposed.find((t) => t.hash === poolTxHash('w0'));
  if (!w0) throw new Error('fixture: w0 is not proposed');

  it('gives its size, its share of a block and the next block it is predicted for', () => {
    expect(predictedFacts({ size: 900 }, snapshot)).toEqual([
      ['size · area', `900 B · ${pct(900, snapshot.consensus.maxBlockBytes)}`],
      ['predicted for', `#${(T + 1).toLocaleString('en-US')}`],
    ]);
  });

  it('opens the tip of a predicted tx, listed in the pool or not', () => {
    const listed = predictedTip({ hash: w0.hash, size: w0.size }, w0, snapshot);
    expect(listed.rows.slice(0, 2)).toEqual(predictedFacts(w0, snapshot));
    expect(listed.rows.slice(2).map(([k]) => k)).toEqual(['proposal in', 'fee rate']);
    const outside = predictedTip({ hash: `0x${'ab'.repeat(32)}`, size: 900 }, null, snapshot);
    expect(outside.rows).toEqual(predictedFacts({ size: 900 }, snapshot));
  });
});
