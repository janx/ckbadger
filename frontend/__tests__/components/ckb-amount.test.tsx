import { describe, expect, it } from 'vitest';
import { render } from '../utils/test-utils';
import { ExactCkb } from '@/components/ui/ckb-amount';
import { MINUS } from '@/lib/chart-ticks';
import { exactCkbText } from '@/lib/ckb-decimal';

describe('ExactCkb', () => {
  it('wraps exact amounts at the decimal or unit without dropping digits', () => {
    const { container } = render(
      <ExactCkb shannons={BigInt('18446744073709551615')} signed={false} wrap />
    );
    const amount = container.querySelector('[data-ckb-amount]');
    expect(amount).toHaveTextContent('184,467,440,737.09551615 CKB');
    expect(amount).toHaveClass('inline-flex', 'flex-wrap', 'justify-end');
    expect(amount?.children[1]).toHaveClass('whitespace-nowrap');
    expect(amount?.children[1]).toHaveTextContent('.09551615');
  });
  it('keeps all 8 decimals, ghosting the trailing zeros, coloured by sign; zero is a dim 0', () => {
    const { container } = render(
      <>
        <ExactCkb shannons={BigInt(-30000)} />
        <ExactCkb shannons={BigInt('75000000000')} />
        <ExactCkb shannons={BigInt(0)} />
      </>
    );
    const [fee, whole, zero] = [...container.querySelectorAll('[data-ckb-amount]')];
    expect(fee).toHaveTextContent(`${MINUS}0.00030000 CKB`);
    expect(fee).toHaveClass('text-negative');
    expect(fee.querySelector('[data-ghost]')).toHaveTextContent(/^0000$/);
    expect(whole).toHaveTextContent('+750.00000000 CKB');
    expect(whole).toHaveClass('text-positive');
    expect(whole.querySelector('[data-ghost]')).toHaveTextContent(/^\.00000000$/);
    // Consistency plan C4 (old: "0.00000000 CKB" in text-text): zero is "0 CKB"; its 0 is a datum
    // in the dim ink (R4, re-pinned on purpose: it was ghosted as if it were padding).
    expect(zero).toHaveTextContent(/^0 CKB$/);
    expect(zero.querySelector('[data-zero]')).toHaveTextContent(/^0$/);
    expect(zero.querySelector('[data-zero]')).toHaveClass('text-text-dim');
    expect(zero.querySelector('[data-ghost]')).toBeNull();
  });

  it('states zero as "0 CKB", its 0 dim, with no fraction for every caller (C4, R4)', () => {
    const { container } = render(
      <>
        <ExactCkb shannons={BigInt(0)} />
        <ExactCkb shannons={BigInt(0)} signed={false} tone="plain" wrap />
        <ExactCkb shannons={BigInt(0)} unit={false} />
      </>
    );
    const [feed, plain, bare] = [...container.querySelectorAll('[data-ckb-amount]')];
    expect(feed).toHaveTextContent(/^0 CKB$/);
    expect(plain).toHaveTextContent(/^0 CKB$/);
    expect(bare).toHaveTextContent(/^0$/);
    for (const amount of [feed, plain, bare]) {
      expect(amount.textContent).not.toContain('.');
      expect(amount.textContent).not.toMatch(/[+−]/);
      const digits = amount.querySelector('[data-zero]');
      expect(digits).toHaveTextContent(/^0$/);
      expect(digits).toHaveClass('text-text-dim');
    }
    // The unit as today: dim, meta, after a space.
    const unit = feed.lastElementChild;
    expect(unit).toHaveTextContent(/^CKB$/);
    expect(unit).toHaveClass('text-text-dim', 'text-meta');
  });

  // UI review 2026-11-14 (DY-06, U-08, ruling R4): the fraction digits are a step quieter than
  // the integer in a named ink of the amount's sign at 4.5:1 (palette.test), never the sign's ink
  // at 78 % (cinnabar 3.7:1 by night, celadon about 3.2–3.7:1 and the neutral 3.5:1 by day).
  it('sets the fraction digits in the quieter ink of the amount’s sign, never at an opacity', () => {
    const { container } = render(
      <>
        <ExactCkb shannons={BigInt(-30000)} />
        <ExactCkb shannons={BigInt('75000000123')} />
        <ExactCkb shannons={BigInt('75000000123')} signed={false} tone="plain" />
        <ExactCkb shannons={BigInt(-30000)} signed={false} tone="plain" />
      </>
    );
    const amounts = [...container.querySelectorAll('[data-ckb-amount]')];
    const fractions = amounts.map((a) => a.querySelector('[data-fraction]'));
    expect(fractions.map((f) => f?.textContent)).toEqual([
      '.0003',
      '.00000123',
      '.00000123',
      '.0003',
    ]);
    expect(fractions[0]).toHaveClass('text-rouge-dim');
    expect(fractions[1]).toHaveClass('text-jade-dim');
    expect(fractions[2]).toHaveClass('text-text-dim');
    expect(fractions[3]).toHaveClass('text-text-dim');
    for (const amount of amounts)
      for (const el of [amount, ...amount.querySelectorAll('*')])
        expect([...el.classList].filter((c) => c.startsWith('opacity-'))).toEqual([]);
  });

  // UI review 2026-11-14 (U-08, ruling R4): a value that IS zero is a datum ("fee 0 CKB"), so it
  // is the dim ink; the ghost is only for the padding zeros of a non-zero amount.
  it('draws a datum zero in the dim ink and only the padding zeros of a non-zero amount ghost', () => {
    const { container } = render(
      <>
        <ExactCkb shannons={BigInt(0)} signed={false} tone="plain" />
        <ExactCkb shannons={BigInt(100000000)} signed={false} tone="plain" />
      </>
    );
    const [zero, one] = [...container.querySelectorAll('[data-ckb-amount]')];
    const datum = zero.querySelector('[data-zero]');
    expect(datum).toHaveTextContent(/^0$/);
    expect(datum).toHaveClass('text-text-dim');
    expect(zero.querySelector('.text-text-ghost')).toBeNull();
    const padding = one.querySelector('[data-ghost]');
    expect(padding).toHaveTextContent(/^\.00000000$/);
    expect(padding).toHaveClass('text-text-ghost');
  });

  // Rubbing × Song print, phase 5 (T4): a sentence that names an amount the page draws (the
  // transaction comment's fee) takes its digits from exactCkbText; it must read as ExactCkb does.
  it('prints the same digits as exactCkbText, signed or not, zero included', () => {
    const amounts = ['0', '1647', '64119', '100000', '100000000', '75000000000', '-30000'];
    for (const signed of [true, false])
      for (const raw of amounts) {
        const shannons = BigInt(raw);
        const { container, unmount } = render(
          <ExactCkb shannons={shannons} signed={signed} tone="plain" />
        );
        expect(container.querySelector('[data-ckb-amount]')!.textContent).toBe(
          `${exactCkbText(shannons, signed)} CKB`
        );
        unmount();
      }
  });
});
