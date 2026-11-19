import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import userEvent from '@testing-library/user-event';
import { render, screen, within } from '@testing-library/react';
import { FilterChips, type FilterValue } from '@/components/ui/filter-chips';
import { atViewportWidth } from '@/__tests__/utils/viewport';
import { inkOf, BRIGHT, QUIET, READING } from '@/__tests__/utils/ink-role';
import { boxPx, targetHeightPx } from '@/__tests__/utils/tailwind-box';

/**
 * The filter chips of the controls board (gate D0′): 28 px high and 32 below 640, square, the frame
 * line; the label 13 px in the reading ink, the count 12 px in the quiet ink, an optional 7 px
 * mark; the selected one the celadon line and the bright ink; a zero's label quiet, never faded.
 * A filter holds one value at a time: a radio group. More than four values below 640 are a select
 * after the filter's label.
 */
type Kind = 'all' | 'ckb' | 'scripts' | 'dao' | 'tokens' | 'objects';
const KINDS: FilterValue<Kind>[] = [
  { key: 'all', label: 'all', count: 6122 },
  { key: 'ckb', label: 'ckb transfers', count: 683, mark: 'jade' },
  { key: 'scripts', label: 'script calls', count: 5425, mark: 'slate-ckb' },
  { key: 'dao', label: 'nervos dao', count: 11, mark: 'gold' },
  { key: 'tokens', label: 'tokens', count: 3, mark: 'token' },
  { key: 'objects', label: 'objects', count: 0, mark: 'object' },
];

function Filter({ values = KINDS }: { values?: FilterValue<Kind>[] }) {
  const [value, setValue] = useState<Kind>('all');
  return (
    <>
      <FilterChips label="kind" values={values} value={value} onChange={setValue} />
      <p>showing {value}</p>
    </>
  );
}

afterEach(() => vi.unstubAllGlobals());

describe('FilterChips: one value of a filter, a radio group', () => {
  it('is a radio group named by the filter, a radio per value named by its words', () => {
    render(<Filter />);
    const group = screen.getByRole('radiogroup', { name: 'kind' });
    expect(
      within(group)
        .getAllByRole('radio')
        .map((r) => r.textContent)
    ).toEqual([
      'all 6,122',
      'ckb transfers 683',
      'script calls 5,425',
      'nervos dao 11',
      'tokens 3',
      'objects 0',
    ]);
    expect(screen.getByRole('radio', { name: 'all 6,122' })).toHaveAttribute(
      'aria-checked',
      'true'
    );
  });

  it('is one stop in the tab order; any arrow moves and chooses, wrapping; Home and End', async () => {
    const user = userEvent.setup();
    render(<Filter />);
    const radio = (name: RegExp) => screen.getByRole('radio', { name });
    expect(screen.getAllByRole('radio').map((r) => r.tabIndex)).toEqual([0, -1, -1, -1, -1, -1]);
    await user.tab();
    expect(radio(/^all/)).toHaveFocus();
    await user.keyboard('{ArrowRight}');
    expect(radio(/^ckb/)).toHaveFocus();
    expect(radio(/^ckb/)).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByText('showing ckb')).toBeInTheDocument();
    await user.keyboard('{ArrowDown}');
    expect(radio(/^script/)).toHaveAttribute('aria-checked', 'true');
    await user.keyboard('{ArrowUp}{ArrowLeft}{ArrowLeft}');
    expect(radio(/^objects/)).toHaveFocus();
    await user.keyboard('{Home}');
    expect(radio(/^all/)).toHaveAttribute('aria-checked', 'true');
    await user.keyboard('{End}');
    expect(radio(/^objects/)).toHaveAttribute('aria-checked', 'true');
  });

  it('chooses on a click', async () => {
    const user = userEvent.setup();
    render(<Filter />);
    await user.click(screen.getByRole('radio', { name: /^nervos dao/ }));
    expect(screen.getByText('showing dao')).toBeInTheDocument();
  });
});

describe('FilterChips: the board’s look', () => {
  it('draws the selected one on the celadon line in the bright ink, the rest on the frame line', () => {
    render(<Filter />);
    const [all, ckb] = screen.getAllByRole('radio');
    expect(all).toHaveClass('border', 'border-jade');
    expect(inkOf(all)).toBe(BRIGHT);
    expect(ckb).toHaveClass('border', 'border-base-border');
    expect(inkOf(ckb)).toBe(READING);
    for (const chip of [all, ckb]) {
      expect(chip.className).not.toMatch(/\bbg-|rounded|opacity/);
      expect(chip).toHaveClass('text-small', 'lowercase');
    }
  });

  it('writes a count 12 px in the quiet ink, a zero’s label quiet and never faded', () => {
    render(<Filter />);
    const count = within(screen.getByRole('radio', { name: /^all/ })).getByText('6,122');
    expect(count).toHaveClass('text-meta');
    expect(inkOf(count)).toBe(QUIET);
    const zero = screen.getByRole('radio', { name: 'objects 0' });
    expect(inkOf(zero)).toBe(QUIET);
    expect(zero.className).not.toMatch(/opacity/);
  });

  it('draws a value’s 7 px mark in its palette colour, unread, and none where it has none', () => {
    render(<Filter />);
    const mark = screen.getByRole('radio', { name: /^ckb/ }).querySelector('[data-mark]')!;
    expect(mark).toHaveAttribute('aria-hidden', 'true');
    expect((mark as HTMLElement).style.backgroundColor).toBe('var(--c-jade)');
    expect(boxPx(mark, 1280)).toBe(7);
    expect(screen.getByRole('radio', { name: /^all/ }).querySelector('[data-mark]')).toBeNull();
  });

  it('stands 28 px high from 640 and 32 below it', () => {
    render(<Filter />);
    const chip = screen.getAllByRole('radio')[0];
    expect([1280, 640, 639, 390, 360].map((w) => targetHeightPx(chip, w))).toEqual([
      28, 28, 32, 32, 32,
    ]);
  });
});

describe('FilterChips: the phone form', () => {
  it('draws more than four values below 640 as a select after the filter’s label', async () => {
    atViewportWidth(390);
    const user = userEvent.setup();
    render(<Filter />);
    expect(screen.queryByRole('radiogroup')).toBeNull();
    const select = screen.getByRole('combobox', { name: 'kind' });
    expect(screen.getAllByRole('option').map((o) => o.textContent)).toEqual([
      'all 6,122',
      'ckb transfers 683',
      'script calls 5,425',
      'nervos dao 11',
      'tokens 3',
      'objects 0',
    ]);
    await user.selectOptions(select, 'dao');
    expect(screen.getByText('showing dao')).toBeInTheDocument();
  });

  it('keeps four values as chips below 640, and six from 640', () => {
    atViewportWidth(390);
    const { unmount } = render(<Filter values={KINDS.slice(0, 4)} />);
    expect(screen.getAllByRole('radio')).toHaveLength(4);
    unmount();
    atViewportWidth(640);
    render(<Filter />);
    expect(screen.getAllByRole('radio')).toHaveLength(6);
  });

  it('refuses a value that is none of its values', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() =>
      render(
        <FilterChips label="kind" values={KINDS} value={'names' as Kind} onChange={() => {}} />
      )
    ).toThrow('FilterChips "kind": no value names');
  });
});
