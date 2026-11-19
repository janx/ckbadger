import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { Dotted, PhoneRow } from '@/components/ui/phone-row';
import {
  ShareBar,
  formatShareTick,
  percentHundredths,
  shareBarWidth,
} from '@/components/ui/share-bar';
import { resourceScale } from '@/components/blocks/block-visual';
import { phoneHeadPx, phoneLines } from '@/__tests__/utils/phone-row';
import { gapPx } from '@/__tests__/utils/tailwind-box';
import { PHONE_PART_GAP_PX } from '@/lib/type-metrics';
import { RowSeal, ROW_SEAL } from '@/components/ui/row-seal';

describe('PhoneRow (boards L1–L8)', () => {
  it('lays out named lines in three voices, each a wrapping left part and a right figure', () => {
    render(
      <PhoneRow
        testId="row"
        lines={[
          { key: 'identity', tone: 'head', left: '#20,895,267', right: '4 txs' },
          { key: 'miner', tone: 'body', left: 'miner …ratvu9', right: null },
          false,
          { key: 'figures', tone: 'quiet', left: <Dotted parts={['3,344 B', null, '0 cycles']} /> },
        ]}
      />
    );
    const row = screen.getByTestId('row');
    expect(phoneLines(row)).toEqual([
      { key: 'identity', tone: 'head', left: '#20,895,267', right: '4 txs' },
      // A null right part draws nothing; a false line is left out.
      { key: 'miner', tone: 'body', left: 'miner …ratvu9', right: null },
      { key: 'figures', tone: 'quiet', left: '3,344 B · 0 cycles', right: null },
    ]);
    const [head, body, quiet] = Array.from(row.children);
    expect(head).toHaveClass('text-body', 'text-text-bright');
    expect(body).toHaveClass('text-small', 'text-text');
    // Quiet figures: 11 px, a step under the body by size; their ink is the reading ink, as a
    // value's (re-pinned with readability ruling R3: it was dim), and an exact amount takes it.
    expect(quiet).toHaveClass('text-meta', 'text-text', '[&_[data-ckb-amount]]:[color:inherit]');
    // Lines wrap rather than truncate; the right figure keeps the right edge when it drops.
    expect(head).toHaveClass('flex-wrap');
    expect(head.querySelector('[data-part="right"]')).toHaveClass('ml-auto', 'shrink-0');
    expect(row.querySelector('.truncate')).toBeNull();
  });

  // Rubbing × Song print, phase 3: the row seal leads the row as its own column beside every line,
  // so the lines keep one left edge; they stay the row's children (phoneLines reads them).
  it('leads the row with a mark in its own column, spanning every line', () => {
    render(
      <PhoneRow
        testId="row"
        mark={<span data-testid="seal">seal</span>}
        lines={[
          { key: 'identity', tone: 'head', left: '#20,895,267', right: '4 txs' },
          false,
          { key: 'miner', tone: 'body', left: 'miner …ratvu9' },
          { key: 'figures', tone: 'quiet', left: '3,344 B' },
        ]}
      />
    );
    const row = screen.getByTestId('row');
    expect(row).toHaveClass('grid', 'grid-cols-[auto_minmax(0,1fr)]', 'gap-y-[3px]');
    const mark = row.firstElementChild as HTMLElement;
    expect(mark).toHaveAttribute('data-part', 'mark');
    expect(mark).toContainElement(screen.getByTestId('seal'));
    expect(mark).toHaveClass('col-start-1', 'self-start');
    // three lines shown (the false one is left out), so the mark spans three rows
    expect(mark.style.gridRow).toBe('1 / span 3');
    expect(phoneLines(row).map((l) => l.key)).toEqual(['identity', 'miner', 'figures']);
    for (const line of Array.from(row.querySelectorAll(':scope > [data-line]')))
      expect(line).toHaveClass('col-start-2');
  });

  // Readability F3, the lead's final overflow sweep: at 360 px `/transactions` was 1 px wider than
  // its window. The committed row's quiet line ends in a list of three figures ("3 → 3 cells ·
  // 1,791 B · 3,124,755 cycles": 40 glyphs, 288 px at the 12 px meta step) and the line beside a
  // 16 px stone is 271.5 px; the right part dropped under the left whole, as it should, but as a
  // part that may not shrink it kept its 288 px and stood 16.6 px past the row. A layout fact, so
  // what this test can hold is the rule that makes it: the right part is never wider than its line
  // (`max-w-full`), so on a line of its own it takes the line's width and its list breaks between
  // its figures. Measured in Chrome with `lanes/checks/phone_right_try.cjs` (the review folder).
  it('never lets a right part be wider than its line', () => {
    render(
      <PhoneRow
        testId="row"
        mark={<span>seal</span>}
        lines={[
          { key: 'identity', tone: 'head', left: '0x8c33…81c4', right: '#21,046,643' },
          {
            key: 'figures',
            tone: 'quiet',
            left: 'fee 0.00001791 CKB',
            right: <Dotted parts={['3 → 3 cells', '1,791 B', '3,124,755 cycles']} />,
          },
        ]}
      />
    );
    const row = screen.getByTestId('row');
    const rights = Array.from(row.querySelectorAll('[data-part="right"]'));
    expect(rights).toHaveLength(2);
    for (const right of rights) expect(right).toHaveClass('ml-auto', 'shrink-0', 'max-w-full');
    // The list inside it is the wrapping one: it can break between its figures.
    const list = rights[1].querySelector('[data-dotted]') as HTMLElement;
    expect(list).toHaveClass('flex', 'flex-wrap', 'min-w-0');
    expect(list.querySelectorAll('[data-dotted-item]')).toHaveLength(3);
  });

  it('draws no mark column when it is given none', () => {
    render(<PhoneRow testId="row" lines={[{ key: 'identity', tone: 'head', left: '#1' }]} />);
    const row = screen.getByTestId('row');
    expect(row).toHaveClass('flex', 'flex-col');
    expect(row).not.toHaveClass('grid');
    expect(row.querySelector('[data-part="mark"]')).toBeNull();
  });

  // Lane D4: a list states its first line (`phoneHead`) with the 12 px between the parts its lines
  // draw, and writes it on the row, so its tests read the drawn line against the stated one.
  it('draws the 12 px between a line’s parts that a stated first line counts', () => {
    render(
      <PhoneRow
        testId="row"
        head={237.6}
        mark={<RowSeal id={{ kind: 'block', hash: `0x${'5e'.repeat(32)}`, txs: 1 }} />}
        lines={[{ key: 'identity', tone: 'head', left: '#21,009,134', right: '1 tx · 20s ago' }]}
      />
    );
    const row = screen.getByTestId('row');
    expect(row).toHaveAttribute('data-head-px', '237.6');
    const line = row.querySelector('[data-line="identity"]')!;
    for (const viewport of [360, 390]) expect(gapPx(line, viewport)).toBe(PHONE_PART_GAP_PX);
    // the stone's footprint (24), the key at 14 px (92.4), the gap and the figure at 13 px (109.2):
    // the board's head
    expect(ROW_SEAL.block.footprint).toBe(24);
    expect(phoneHeadPx(row, 360)).toBe(237.6);
  });

  // Lane D4 (PB-09): a line that holds its right part keeps it at the line's right edge, so where a
  // chip lands does not depend on how long the left part is; the left wraps beside it, between the
  // items of its separated list.
  // renamed (lane p-assets): it was "holds a right part at its line’s edge where the line says so,
  // and wraps the rest otherwise"
  it('holds a right part at its line’s edge where it fits beside the left’s widest item, else puts it under', () => {
    render(
      <PhoneRow
        testId="row"
        lines={[
          { key: 'identity', tone: 'head', left: 'Wyltek Founding Members', right: '100 CKB' },
          {
            key: 'what',
            tone: 'body',
            hold: true,
            left: <Dotted parts={['Spore', '93 objects', '32 holders']} />,
            right: 'Pure CKB',
          },
        ]}
      />
    );
    const row = screen.getByTestId('row');
    const [head, what] = Array.from(row.querySelectorAll(':scope > [data-line]'));
    expect(head).toHaveClass('flex-wrap');
    expect(head).not.toHaveClass('flex-nowrap');
    // Re-pinned on purpose (lane p-assets; review of phases C and D, logic SHOULD 4: a hold line's
    // "must fit together" was checked nowhere, and at 360 a token holder's share, 202.4 px that
    // cannot wrap, beside "UTXOSwap Proxy Lock", 148.2 px, made 362.6 px on a 328 px line: the share
    // was drawn over the lock): a hold line wraps too, by its own rule. Its left part's flex basis
    // is nothing and its least width its widest unbreakable item (min-content), so the right part
    // stays on the line wherever that item and the right part fit together, the left growing beside
    // it and wrapping there whatever its length; where they do not, the right part goes under,
    // at the right edge (it was flex-nowrap, the left part shrinking under its own content).
    expect(what).toHaveClass('flex-wrap');
    expect(what).not.toHaveClass('flex-nowrap');
    const left = what.querySelector(':scope > [data-part="left"]');
    expect(left).toHaveClass('basis-0', 'grow', 'min-w-min');
    expect(left).not.toHaveClass('min-w-0');
    expect(what.querySelector('[data-dotted]')).toHaveClass('flex-wrap');
    expect(what.querySelector(':scope > [data-part="right"]')).toHaveClass(
      'shrink-0',
      'ml-auto',
      'max-w-full'
    );
    // a line without hold keeps its left part's own width, shrinking under it
    expect(head.querySelector(':scope > [data-part="left"]')).toHaveClass('min-w-0');
    expect(head.querySelector(':scope > [data-part="left"]')).not.toHaveClass('basis-0');
  });

  it('refuses a row without a line', () => {
    expect(() => render(<PhoneRow lines={[false, null]} />)).toThrow('at least one line');
  });
});

// Re-pinned with ruling R7 (UI review 2026-11-14): the 10 % = full clamp of board L5 is gone; a
// bar is drawn on its list's one scale, the table's rounded maximum.
describe('Share bar (ruling R7: the rounded maximum of the list, no clamp)', () => {
  it('draws a value on its scale, in proportion, with no clamp', () => {
    expect(shareBarWidth(232, 1000)).toBe('23.2%');
    expect(shareBarWidth(5273, 6000)).toBe('87.88%');
    expect(shareBarWidth(2645, 6000)).toBe('44.08%');
    expect(shareBarWidth(6000, 6000)).toBe('100%');
    expect(shareBarWidth(0, 1)).toBe('0%');
    expect(() => shareBarWidth(11, 10)).toThrow('is not on the scale 10');
    expect(() => shareBarWidth(1, 0)).toThrow('invalid scale 0');
    const { container } = render(
      <ShareBar value={232} scale={1000} track={120} color="var(--c-token)" />
    );
    const track = container.firstElementChild as HTMLElement;
    expect(track.style.width).toBe('120px');
    expect((track.firstElementChild as HTMLElement).style.width).toBe('23.2%');
  });

  it('reads an API percentage as hundredths and names the rounded maximum for a few maxima', () => {
    expect(percentHundredths('2.32')).toBe(232);
    expect(percentHundredths('1')).toBe(100);
    expect(percentHundredths('42.5')).toBe(4250);
    expect(() => percentHundredths('1.234')).toThrow('is not a percentage');
    expect(() => percentHundredths('101')).toThrow('is not a share');
    // the smallest round percentage at or above the largest share on the page
    const axis = (largest: string) => formatShareTick(resourceScale([percentHundredths(largest)]));
    expect(axis('52.73')).toBe('60%');
    expect(axis('8.23')).toBe('10%');
    expect(axis('25')).toBe('30%');
    expect(axis('1.00')).toBe('1%');
    expect(axis('0.04')).toBe('0.04%');
    expect(axis('100')).toBe('100%');
  });
});
