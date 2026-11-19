import { describe, expect, it } from 'vitest';
import { render, screen } from '../utils/test-utils';
import { FiberSplitBar } from '@/components/fiber/fiber-split-bar';

describe('FiberSplitBar (What moved inside the channel, FF-C3)', () => {
  it('splits one bar by the two exact amounts, first party grey, second jade', () => {
    const { container } = render(
      <FiberSplitBar
        left={{ name: 'ppedrr', shannons: BigInt('100000000569') }}
        right={{ name: '5eyy6y', shannons: BigInt('6200000351') }}
        note="at open"
      />
    );
    const left = container.querySelector('[data-split="left"]') as HTMLElement;
    const right = container.querySelector('[data-split="right"]') as HTMLElement;
    expect(parseFloat(left.style.width)).toBeCloseTo((1000.00000569 / 1062.0000092) * 100, 3);
    expect(left.style.background).toBe('rgb(var(--rgb-text) / 0.3)');
    expect(right.style.background).toBe('rgb(var(--rgb-jade) / 0.28)');
    expect(screen.getByText('ppedrr')).toBeInTheDocument();
    expect(
      Array.from(container.querySelectorAll('[data-ckb-amount]')).map((el) => el.textContent)
    ).toEqual(['1,000.00000569 CKB', '62.00000351 CKB']);
    expect(screen.getByText('at open')).toBeInTheDocument();
  });

  // Lane P4 (readability R3, R5): a figure's labels are its values, read out exact (no padding to
  // eight decimals, "699.00000000" was a column's form in a figure), and a party's id is a value,
  // in the reading ink (it was the quiet ink)
  it('writes its two amounts exact, with no padding, and its names in the reading ink', () => {
    const { container } = render(
      <FiberSplitBar
        left={{ name: 'ppedrr', shannons: BigInt('69900000000') }}
        right={{ name: '5eyy6y', shannons: BigInt('30100000000') }}
        note="at open"
      />
    );
    expect(
      Array.from(container.querySelectorAll('[data-ckb-amount]')).map((el) => el.textContent)
    ).toEqual(['699 CKB', '301 CKB']);
    for (const name of ['ppedrr', '5eyy6y']) {
      expect(screen.getByText(name)).toHaveClass('text-text');
      expect(screen.getByText(name)).not.toHaveClass('text-text-dim');
    }
  });

  it('refuses a split it cannot draw: a side that put nothing in', () => {
    expect(() =>
      render(
        <FiberSplitBar
          left={{ name: 'a', shannons: BigInt(0) }}
          right={{ name: 'b', shannons: BigInt(5) }}
          note=""
        />
      )
    ).toThrow('Fiber split bar needs two positive amounts');
  });
});
