import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import userEvent from '@testing-library/user-event';
import { render, screen } from '@testing-library/react';
import { RangeSelect } from '@/components/ui/range-select';
import { inkOf, BRIGHT, READING } from '@/__tests__/utils/ink-role';
import { targetHeightPx } from '@/__tests__/utils/tailwind-box';

/**
 * The range of the controls board (gate D0′): the filter chips joined, one value at a time (a
 * radio group); one spelling of a span — a number, a space, a lowercase unit — and `all`.
 */
const SPANS = [
  { key: '7d', label: '1 w' },
  { key: '30d', label: '1 m' },
  { key: '365d', label: '1 y' },
  { key: 'all', label: 'all' },
] as const;
type Span = (typeof SPANS)[number]['key'];

function Range() {
  const [value, setValue] = useState<Span>('all');
  return (
    <>
      <RangeSelect label="range" options={SPANS} value={value} onChange={setValue} />
      <p>over {value}</p>
    </>
  );
}

afterEach(() => vi.restoreAllMocks());

describe('RangeSelect', () => {
  it('is a radio group of spans, the chosen one checked', () => {
    render(<Range />);
    expect(screen.getByRole('radiogroup', { name: 'range' })).toBeInTheDocument();
    expect(screen.getAllByRole('radio').map((r) => r.textContent)).toEqual([
      '1 w',
      '1 m',
      '1 y',
      'all',
    ]);
    expect(screen.getByRole('radio', { name: 'all' })).toHaveAttribute('aria-checked', 'true');
  });

  it('moves and chooses with any arrow, Home and End; one stop in the tab order', async () => {
    const user = userEvent.setup();
    render(<Range />);
    expect(screen.getAllByRole('radio').map((r) => r.tabIndex)).toEqual([-1, -1, -1, 0]);
    await user.tab();
    await user.keyboard('{ArrowRight}');
    expect(screen.getByRole('radio', { name: '1 w' })).toHaveFocus();
    expect(screen.getByText('over 7d')).toBeInTheDocument();
    await user.keyboard('{ArrowDown}');
    expect(screen.getByText('over 30d')).toBeInTheDocument();
    await user.keyboard('{End}');
    expect(screen.getByRole('radio', { name: 'all' })).toHaveAttribute('aria-checked', 'true');
    await user.keyboard('{Home}');
    expect(screen.getByText('over 7d')).toBeInTheDocument();
  });

  it('joins the chips: each after the first overlaps the line before it, the chosen one on top', () => {
    render(<Range />);
    const [first, ...rest] = screen.getAllByRole('radio');
    expect(first.className).not.toMatch(/-ml-px/);
    for (const chip of rest) expect(chip).toHaveClass('-ml-px');
    const chosen = screen.getByRole('radio', { name: 'all' });
    expect(chosen).toHaveClass('border-jade', 'relative', 'z-[1]');
    expect(inkOf(chosen)).toBe(BRIGHT);
    expect(inkOf(first)).toBe(READING);
    expect([1280, 390].map((w) => targetHeightPx(first, w))).toEqual([28, 32]);
  });

  it('takes the board’s three label sets', () => {
    for (const labels of [
      ['1 w', '1 m', '1 y', 'all'],
      ['30 d', '90 d', '1 y', 'all'],
      ['24 h', '14 d'],
    ]) {
      const { unmount } = render(
        <RangeSelect
          label="range"
          options={labels.map((label) => ({ key: label, label }))}
          value={labels[0]}
          onChange={() => {}}
        />
      );
      expect(screen.getAllByRole('radio').map((r) => r.textContent)).toEqual(labels);
      unmount();
    }
  });

  it('refuses a label that is not the one spelling of a span', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    for (const label of ['1W', '1w', '24H', '30 days', 'All', ' 1 y', '1  y', 'max', '0.5 d'])
      expect(
        () =>
          render(
            <RangeSelect
              label="range"
              options={[{ key: 'x', label }]}
              value="x"
              onChange={() => {}}
            />
          ),
        label
      ).toThrow(`RangeSelect "range": "${label}" is not a span`);
  });
});
