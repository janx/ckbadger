import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import userEvent from '@testing-library/user-event';
import { render, screen } from '@testing-library/react';
import { Select } from '@/components/ui/select';
import { inkOf, QUIET, READING } from '@/__tests__/utils/ink-role';
import { targetHeightPx } from '@/__tests__/utils/tailwind-box';

/**
 * The select of the controls board (gate D0′): a real <select> in the text field's well, 28 px
 * and 32 below 640, 13 px; it always stands after its label, which is quiet; its chosen value is
 * the reading ink.
 */
const SORTS = [
  { value: 'capacity', label: 'ckb held in its cells' },
  { value: 'holders', label: 'holders' },
  { value: 'name', label: 'name' },
] as const;
type Sort = (typeof SORTS)[number]['value'];

function Sorted() {
  const [value, setValue] = useState<Sort>('capacity');
  return <Select label="sorted by" value={value} options={SORTS} onChange={setValue} />;
}

afterEach(() => vi.restoreAllMocks());

describe('Select', () => {
  it('is a real select named by the label it stands after', () => {
    render(<Sorted />);
    const select = screen.getByRole('combobox', { name: 'sorted by' });
    expect(select.tagName).toBe('SELECT');
    const label = screen.getByText('sorted by');
    expect(label.tagName).toBe('LABEL');
    // the label first, then the select's well
    expect(label.compareDocumentPosition(select) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(screen.getAllByRole('option').map((o) => o.textContent)).toEqual([
      'ckb held in its cells',
      'holders',
      'name',
    ]);
  });

  it('writes its label in the quiet ink and its chosen value in the reading ink, 13 px', () => {
    render(<Sorted />);
    const select = screen.getByRole('combobox');
    expect(inkOf(screen.getByText('sorted by'))).toBe(QUIET);
    expect(inkOf(select)).toBe(READING);
    expect(select).toHaveClass('text-small');
    expect(screen.getByText('sorted by')).toHaveClass('text-small');
  });

  it('answers a choice', async () => {
    const user = userEvent.setup();
    render(<Sorted />);
    await user.selectOptions(screen.getByRole('combobox'), 'holders');
    expect(screen.getByRole('combobox')).toHaveValue('holders');
  });

  it('stands in the field’s well, 28 px high from 640 and 32 below, its chevron unread', () => {
    const { container } = render(<Sorted />);
    const well = screen.getByRole('combobox').parentElement!;
    expect(well).toHaveClass('bg-base-void', 'border-base-border');
    expect([1280, 640, 639, 390].map((w) => targetHeightPx(well, w))).toEqual([28, 28, 32, 32]);
    expect(container.querySelector('svg')).toHaveAttribute('aria-hidden', 'true');
  });

  it('refuses a value none of its options holds', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() =>
      render(<Select label="sorted by" value={'age' as Sort} options={SORTS} onChange={() => {}} />)
    ).toThrow('Select "sorted by": no option age');
  });
});
