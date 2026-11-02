import { fireEvent, render, screen, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { CapacityRangeSelector } from '@/components/ui/capacity-range-selector';

describe('CapacityRangeSelector', () => {
  it('keeps the boxed jade selector by default (address and token pages)', () => {
    const { container } = render(<CapacityRangeSelector value="1y" onChange={() => {}} />);
    const active = screen.getByRole('button', { name: '1Y' });
    expect(active).toHaveClass('text-jade', 'border-jade-dim');
    expect(container.firstElementChild).toHaveClass('flex', 'border');
    expect(screen.queryByRole('group')).toBeNull();
  });

  // Hierarchy v3 (R11, R13; lane C): on a title row the range is one inline group of buttons and
  // the pressed one is neutral, as the DAO page's range — no jade text in tier 1.
  it('draws a quiet inline group with a neutral pressed button on a title row', () => {
    const onChange = vi.fn();
    render(<CapacityRangeSelector value="all" onChange={onChange} quiet />);
    const group = screen.getByRole('group', { name: 'Range' });
    expect(group).toHaveClass('whitespace-nowrap');
    const buttons = within(group).getAllByRole('button');
    expect(buttons.map((b) => b.textContent)).toEqual(['30D', '90D', '1Y', 'ALL']);
    const all = within(group).getByRole('button', { name: 'ALL' });
    expect(all).toHaveAttribute('aria-pressed', 'true');
    expect(all).toHaveClass('text-text-bright', 'inline-flex');
    for (const b of buttons) expect(b.className).not.toMatch(/jade/);
    fireEvent.click(within(group).getByRole('button', { name: '90D' }));
    expect(onChange).toHaveBeenCalledWith('90d');
  });
});
