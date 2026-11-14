import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen, within } from '@testing-library/react';
import { T0, tick } from '@/__tests__/utils/still-clock';
import {
  FIXTURE_NOW,
  FIXTURE_TIP,
  buildPipelineSnapshot,
  pipelineSnapshot,
  poolTxHash,
} from '@/__tests__/msw/fixtures/pipeline-snapshot';
import type { PipelineBlock, PipelineSnapshot } from '@/lib/api';
import {
  PipelineTooltip,
  pct,
  cellbaseTip,
  txTip,
  uncleTip,
  windowTip,
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
    expect(pct(999_999, 1_000_000)).toBe('>99.99%');
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
    const tip = txTip(
      { kind: 'pending', tx: { ...tx, fee: '9007199254740993' } },
      snapshot,
      FIXTURE_NOW
    );
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
    const tip = renderTip(
      txTip({ kind: 'proposed', tx: early, tooEarly: true }, snapshot, FIXTURE_NOW)
    );
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
      txTip({ kind: 'pending', tx: { ...pool.pending[0], arrivedAt: T0 - 20_000 } }, snapshot, T0)
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
