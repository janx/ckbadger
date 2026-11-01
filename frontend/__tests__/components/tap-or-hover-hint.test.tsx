import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { TapOrHoverHint, tapOrHover } from '@/components/ui/tap-or-hover-hint';
import { ReadoutRow } from '@/components/ui/readout-row';

describe('the tap-or-hover hint (R6: one wording for every hover target)', () => {
  it('says "tap or hover", with what to tap, capitalised at a sentence start', () => {
    expect(tapOrHover()).toBe('tap or hover');
    expect(tapOrHover('a day')).toBe('tap or hover a day');
    expect(tapOrHover('a party or a pair.', { capital: true })).toBe(
      'Tap or hover a party or a pair.'
    );
    expect(() => tapOrHover(' ')).toThrow('tap-or-hover hint: an empty target');
  });

  it('renders the same words inline, marked as the shared hint', () => {
    render(
      <p>
        dots: when each version was deployed · <TapOrHoverHint what="for its name" />
      </p>
    );
    const hint = screen.getByText('tap or hover for its name');
    expect(hint.tagName).toBe('SPAN');
    expect(hint).toHaveAttribute('data-hint', 'tap-or-hover');
    expect(hint.parentElement).toHaveTextContent(
      'dots: when each version was deployed · tap or hover for its name'
    );
  });

  it('is the hint the readout line under a figure offers', () => {
    render(
      <ReadoutRow label="Day readout" pinned={false}>
        {null}
      </ReadoutRow>
    );
    const hint = screen.getByText('tap or hover');
    expect(hint).toHaveAttribute('data-hint', 'tap-or-hover');
    expect(hint).toHaveClass('whitespace-nowrap', 'text-[11px]');
  });
});
