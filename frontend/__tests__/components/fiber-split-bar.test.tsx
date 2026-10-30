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
    expect(left.style.background).toBe('rgba(160, 168, 190, 0.3)');
    expect(right.style.background).toBe('rgba(46, 219, 163, 0.28)');
    expect(screen.getByText('ppedrr')).toBeInTheDocument();
    expect(
      Array.from(container.querySelectorAll('[data-ckb-amount]')).map((el) => el.textContent)
    ).toEqual(['1,000.00000569 CKB', '62.00000351 CKB']);
    expect(screen.getByText('at open')).toBeInTheDocument();
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
