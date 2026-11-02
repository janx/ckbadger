import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { Dotted, PhoneRow } from '@/components/ui/phone-row';
import { ShareBar, shareBarWidth, shareBarWidthFromPercent } from '@/components/ui/share-bar';
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

  it('refuses a row without a line', () => {
    expect(() => render(<PhoneRow lines={[false, null]} />)).toThrow('at least one line');
  });
});

describe('Share bar (board L5: 10 % = full)', () => {
  it('draws a share against 10 % from exact integers, and a full bar from 10 % up', () => {
    expect(shareBarWidth(BigInt(232), BigInt(10000))).toBe('23.2%');
    expect(shareBarWidth(BigInt(1), BigInt(10))).toBe('100%');
    expect(shareBarWidth(BigInt(9), BigInt(10))).toBe('100%');
    expect(shareBarWidth(BigInt(0), BigInt(0))).toBe('0%');
    expect(() => shareBarWidth(BigInt(11), BigInt(10))).toThrow('is not a share');
    // An API's exact percentage text, read as hundredths.
    expect(shareBarWidthFromPercent('2.32')).toBe('23.2%');
    expect(shareBarWidthFromPercent('1')).toBe('10%');
    expect(shareBarWidthFromPercent('42.5')).toBe('100%');
    expect(() => shareBarWidthFromPercent('1.234')).toThrow('is not a percentage');
    expect(() => shareBarWidthFromPercent('101')).toThrow('is not a share');
    const { container } = render(<ShareBar width="23.2%" />);
    expect((container.firstElementChild!.firstElementChild as HTMLElement).style.width).toBe(
      '23.2%'
    );
  });
});
