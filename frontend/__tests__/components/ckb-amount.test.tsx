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

// Readability plan 2026-11-18, ruling R5 (lane B2): one renderer, four forms, the form named by
// every call site; the whole amount in one ink (the sign's with tone="sign", the reading ink with
// tone="plain", its parent's with tone="inherit"), the ghost only for a column's padding; a
// shortened figure carries its exact value and unit in its title.
describe('the forms of a CKB amount (R5)', () => {
  const CKB = BigInt(100000000);
  const amountOf = (container: HTMLElement) =>
    container.querySelector('[data-ckb-amount]') as HTMLElement;
  /** The ink classes inside an amount, but the unit's and the padding's. */
  const innerInks = (amount: HTMLElement) =>
    [...amount.querySelectorAll('*')]
      .filter((el) => !el.hasAttribute('data-ghost') && el.textContent !== ' CKB')
      .flatMap((el) =>
        [...el.classList].filter((c) => /^text-(text|jade|rouge|positive|negative)/.test(c))
      );

  it('summary: whole CKB from 1,000 up, the exact value and its unit in the title', () => {
    const { container } = render(
      <ExactCkb form="summary" shannons={BigInt('100099999999')} signed={false} tone="plain" />
    );
    const amount = amountOf(container);
    expect(amount).toHaveTextContent(/^1,000 CKB$/);
    expect(amount).toHaveAttribute('title', '1,000.99999999 CKB');
  });

  it('summary: two decimals at most from 1 to 1,000, and no title when nothing was cut', () => {
    const { container } = render(
      <>
        <ExactCkb form="summary" shannons={BigInt(1250000000)} signed={false} tone="plain" />
        <ExactCkb form="summary" shannons={BigInt(99999500000)} signed={false} tone="plain" />
        <ExactCkb form="summary" shannons={BigInt(30000)} signed={false} tone="plain" />
      </>
    );
    const [whole, cut, small] = [...container.querySelectorAll('[data-ckb-amount]')];
    expect(whole).toHaveTextContent(/^12\.5 CKB$/);
    expect(whole).not.toHaveAttribute('title');
    expect(cut).toHaveTextContent(/^999\.99 CKB$/);
    expect(cut).toHaveAttribute('title', '999.995 CKB');
    expect(small).toHaveTextContent(/^0\.0003 CKB$/);
    expect(small).not.toHaveAttribute('title');
  });

  it('summary: a signed amount keeps its sign in the figure and in the title', () => {
    const { container } = render(<ExactCkb form="summary" shannons={-BigInt('123456789012')} />);
    const amount = amountOf(container);
    expect(amount).toHaveTextContent(`${MINUS}1,234 CKB`);
    expect(amount).toHaveAttribute('title', `${MINUS}1,234.56789012 CKB`);
    expect(amount).toHaveClass('text-negative');
  });

  it('exact: the value with its trailing zeros dropped, no point for a whole number', () => {
    const { container } = render(
      <>
        <ExactCkb form="exact" shannons={BigInt(122000) * CKB} signed={false} tone="plain" />
        <ExactCkb form="exact" shannons={BigInt(30000)} signed={false} tone="plain" />
        <ExactCkb form="exact" shannons={BigInt(55951253210)} signed={false} tone="plain" />
      </>
    );
    const amounts = [...container.querySelectorAll('[data-ckb-amount]')];
    expect(amounts.map((a) => a.textContent)).toEqual([
      '122,000 CKB',
      '0.0003 CKB',
      '559.5125321 CKB',
    ]);
    for (const a of amounts) {
      expect(a).not.toHaveAttribute('title');
      expect(a.querySelector('[data-ghost]')).toBeNull();
    }
  });

  it('column: exact, padded to 8 decimals, the padding the only ghost', () => {
    const { container } = render(
      <ExactCkb form="column" shannons={BigInt(75000000000)} signed={false} tone="plain" />
    );
    const amount = amountOf(container);
    expect(amount).toHaveTextContent(/^750\.00000000 CKB$/);
    expect(amount.querySelector('[data-ghost]')).toHaveTextContent(/^\.00000000$/);
    expect(amount).not.toHaveAttribute('title');
  });

  it('compact: two decimals of the unit reached, cut, the exact value in the title', () => {
    const { container } = render(
      <ExactCkb
        form="compact"
        shannons={BigInt('902451123450000000')}
        signed={false}
        tone="plain"
      />
    );
    const amount = amountOf(container);
    expect(amount).toHaveTextContent(/^9\.02B CKB$/);
    expect(amount).toHaveAttribute('title', '9,024,511,234.5 CKB');
  });

  it('draws the whole amount in one ink, in every form and every tone', () => {
    for (const form of ['exact', 'column', 'summary', 'compact'] as const)
      for (const [shannons, tone, ink] of [
        [BigInt('-75000000123'), 'sign', 'text-negative'],
        [BigInt('75000000123'), 'sign', 'text-positive'],
        [BigInt('75000000123'), 'plain', 'text-text'],
      ] as const) {
        const { container, unmount } = render(
          <ExactCkb form={form} shannons={shannons} tone={tone} />
        );
        const amount = amountOf(container);
        expect(amount, `${form} ${tone}`).toHaveClass(ink);
        expect(innerInks(amount), `${form} ${tone}`).toEqual([]);
        unmount();
      }
  });

  it('leaves the ink to its parent with tone="inherit"', () => {
    const { container } = render(
      <span className="text-gold">
        <ExactCkb form="summary" shannons={BigInt('75000000123')} signed={false} tone="inherit" />
      </span>
    );
    const amount = amountOf(container);
    expect([...amount.classList].filter((c) => /^text-(text|positive|negative)/.test(c))).toEqual(
      []
    );
    expect(innerInks(amount)).toEqual([]);
  });

  it('states zero as a dim 0 in every form', () => {
    for (const form of ['exact', 'column', 'summary', 'compact'] as const) {
      const { container, unmount } = render(<ExactCkb form={form} shannons={BigInt(0)} />);
      const amount = amountOf(container);
      expect(amount).toHaveTextContent(/^0 CKB$/);
      expect(amount.querySelector('[data-zero]')).toHaveClass('text-text-dim');
      expect(amount).not.toHaveAttribute('title');
      unmount();
    }
  });
});
