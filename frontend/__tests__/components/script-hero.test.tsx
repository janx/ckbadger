import { describe, expect, it } from 'vitest';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { CkbFigure, HeroFigure } from '@/components/scripts/script-hero';

/** 49,890,081,314.93777532 CKB, the scripts list's figure in the review's crop. */
const FIGURE = BigInt('4989008131493777532');

describe('the scripts pages’ display figure (CkbFigure)', () => {
  // PA-19 (UI review 2026-11-14): "49,890,081,314.93777532" filled a phone's line and "CKB" dropped
  // to the next (the scripts list, the script pages). Below 640 the figure is 26 px: the integer,
  // 14 characters, 218.4 px; the decimals at 22 px, 9 characters, 118.8; " CKB" at 13 px, 31.2:
  // 368.4 px, wider than the 358 px line at 390 and the 326 at 360. One step down (17 px, 91.8)
  // makes 341.4, still over 326; at 13 px (70.2) the pair takes 319.8 and fits at 360. So below
  // 640 the decimals are 13 px (from 640 22 px, as before), and the figure and its unit are one
  // unbreakable unit at every width.
  it('keeps the figure and its unit one unit, the decimals 13 px below 640 (PA-19)', () => {
    const { container } = render(<CkbFigure shannons={FIGURE} />);
    const amount = container.querySelector('[data-role="figure-amount"]') as HTMLElement;
    expect(amount).toHaveClass('text-index', 'sm:text-display', 'text-text-bright');
    expect(amount).toHaveTextContent(/^49,890,081,314\.93777532 CKB$/);
    expect(amount.children).toHaveLength(1);
    const whole = amount.firstElementChild as HTMLElement;
    expect(whole).toHaveClass('whitespace-nowrap');
    expect(whole).toHaveTextContent(/^49,890,081,314\.93777532 CKB$/);
    const decimals = within(whole).getByText('.93777532');
    expect(decimals).toHaveClass('text-body', 'sm:text-hero', 'text-text');
    expect(decimals).not.toHaveClass('text-hero');
    expect(within(whole).getByText('CKB')).toHaveClass('text-body', 'text-text-dim');
  });

  it('keeps the unit with the figure in the hero block too', () => {
    render(
      <HeroFigure label="CKB under these scripts" shannons={FIGURE} companion={['in 3 cells']} />
    );
    const unit = screen.getByText('CKB');
    expect(unit.closest('.whitespace-nowrap')).toHaveTextContent(/^49,890,081,314\.93777532 CKB$/);
  });
});
