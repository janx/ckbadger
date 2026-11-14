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
import { phoneLines } from '@/__tests__/utils/phone-row';

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
    // Quiet figures: 11 px dim, and an exact amount takes the line's ink.
    expect(quiet).toHaveClass(
      'text-meta',
      'text-text-dim',
      '[&_[data-ckb-amount]]:[color:inherit]'
    );
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

  it('draws no mark column when it is given none', () => {
    render(<PhoneRow testId="row" lines={[{ key: 'identity', tone: 'head', left: '#1' }]} />);
    const row = screen.getByTestId('row');
    expect(row).toHaveClass('flex', 'flex-col');
    expect(row).not.toHaveClass('grid');
    expect(row.querySelector('[data-part="mark"]')).toBeNull();
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
    const { container } = render(<ShareBar value={232} scale={1000} track={120} />);
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
