import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import userEvent from '@testing-library/user-event';
import { render, screen, within } from '@testing-library/react';
import { FilterChips, type FilterValue } from '@/components/ui/filter-chips';
import { atViewportWidth } from '@/__tests__/utils/viewport';
import { inkOf, BRIGHT, QUIET, READING } from '@/__tests__/utils/ink-role';
import { boxPx, contentPx, targetHeightPx } from '@/__tests__/utils/tailwind-box';

/**
 * The filter chips of the controls board (gate D0′): 28 px high and 32 below 640, square, the frame
 * line; the label 13 px in the reading ink, the count 12 px in the quiet ink, an optional 7 px
 * mark; the selected one the celadon line and the bright ink; a zero's label quiet, never faded.
 * A filter holds one value at a time: a radio group. Below 640 a row of chips that does not fit
 * one line of the narrowest phone is a select after the filter's label.
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

  // Lane c2-chain (added prop): a value whose count is not exact draws no number, and its chip
  // says why on hover (the address page's activity kinds: a filtered page tells 0, 1 or more).
  it('puts a value’s title on its chip, and none where it has none', () => {
    render(
      <Filter
        values={[
          KINDS[0],
          { ...KINDS[1], count: undefined, title: 'more than one; no count is kept' },
          ...KINDS.slice(2),
        ]}
      />
    );
    const ckb = screen.getByRole('radio', { name: 'ckb transfers' });
    expect(ckb).toHaveAttribute('title', 'more than one; no count is kept');
    expect(ckb.textContent).toBe('ckb transfers');
    expect(screen.getByRole('radio', { name: /^all/ })).not.toHaveAttribute('title');
  });

  it('stands 28 px high from 640 and 32 below it', () => {
    render(<Filter />);
    const chip = screen.getAllByRole('radio')[0];
    expect([1280, 640, 639, 390, 360].map((w) => targetHeightPx(chip, w))).toEqual([
      28, 28, 32, 32, 32,
    ]);
  });
});

// Re-pinned on purpose (the coordinator's follow-up of lane C1): the phone form is decided by
// measurement (lib/control-row-fit.ts `chipRowFits`: the row on one 328 px line), no longer by a
// count of values, as the board's pictures at 390 draw it: the address page's three kinds are
// chips, the inventory's four standards (397.6 px) and the six kinds (783.2 px) a select.
const ADDRESS_KINDS: FilterValue<string>[] = [
  { key: 'all', label: 'all', count: 3 },
  { key: 'ckb', label: 'ckb', count: 2 },
  { key: 'dao', label: 'dao', count: 1, mark: 'gold' },
];
const STANDARDS: FilterValue<string>[] = [
  { key: 'all', label: 'all', count: 727 },
  { key: 'xudt', label: 'xudt', count: 629 },
  { key: 'sudt', label: 'sudt', count: 94 },
  { key: 'compatible', label: 'xudt-compatible', count: 4 },
];
const INSIDE: FilterValue<string>[] = [
  { key: 'all', label: 'all', count: 1234 },
  { key: 'tokens', label: 'tokens' },
  { key: 'objects', label: 'objects' },
  { key: 'domains', label: 'domains' },
];
const OVER: FilterValue<string>[] = [
  { key: 'all', label: 'all', count: 727 },
  { key: 'tokens', label: 'tokens' },
  { key: 'objects', label: 'objects' },
  { key: 'addresses', label: 'addresses' },
];

function Values({ label, values }: { label: string; values: FilterValue<string>[] }) {
  const [value, setValue] = useState('all');
  return <FilterChips label={label} values={values} value={value} onChange={setValue} />;
}

/** The form a value set takes at a viewport: chips, or the select after its label. */
function formAt(viewport: number, values: FilterValue<string>[], label = 'standard') {
  atViewportWidth(viewport);
  const { unmount } = render(<Values label={label} values={values} />);
  const form = screen.queryByRole('radiogroup')
    ? 'chips'
    : screen.queryByRole('combobox', { name: label })
      ? 'select'
      : 'none';
  unmount();
  return form;
}

describe('FilterChips: the phone form, by the one function (lib/control-row-fit)', () => {
  it('draws a row wider than the narrowest phone’s line as a select after the filter’s label', async () => {
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

  it('draws the board’s three cases at 390: the address page’s chips, the two selects', () => {
    expect(formAt(390, ADDRESS_KINDS, 'kind')).toBe('chips');
    expect(formAt(390, STANDARDS)).toBe('select');
    expect(formAt(390, KINDS, 'kind')).toBe('select');
  });

  it('keeps a row 0.6 px inside the line as chips, and draws one 0.6 px over it as a select', () => {
    expect(formAt(390, INSIDE)).toBe('chips');
    expect(formAt(360, INSIDE)).toBe('chips');
    expect(formAt(390, OVER)).toBe('select');
  });

  it('draws every value set as chips from 640', () => {
    for (const values of [ADDRESS_KINDS, STANDARDS, KINDS, OVER])
      expect(formAt(640, values)).toBe('chips');
  });

  it('draws the row the function measures', () => {
    atViewportWidth(1280);
    for (const [values, px] of [
      [ADDRESS_KINDS, 200.8],
      [STANDARDS, 397.6],
      [KINDS, 783.2],
    ] as const) {
      const { unmount } = render(<Values label="kind" values={[...values]} />);
      const row = screen.getByRole('radiogroup');
      expect(Math.round(contentPx(row, 1280) * 100) / 100).toBe(px);
      unmount();
    }
  });

  it('refuses a value that is none of its values', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() =>
      render(
        <FilterChips label="kind" values={KINDS} value={'names' as Kind} onChange={() => {}} />
      )
    ).toThrow('FilterChips "kind": no value names');
  });

  it('refuses a filter with no label, at every width: its select would stand without one', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    for (const viewport of [1280, 390]) {
      atViewportWidth(viewport);
      expect(() =>
        render(<FilterChips label="" values={STANDARDS} value="all" onChange={() => {}} />)
      ).toThrow('FilterChips: a filter has a label');
    }
  });
});
