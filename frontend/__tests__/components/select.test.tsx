import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import userEvent from '@testing-library/user-event';
import { render, screen } from '@testing-library/react';
import { Select } from '@/components/ui/select';
import { selectPairPx } from '@/lib/control-row-fit';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { inkOf, QUIET, READING } from '@/__tests__/utils/ink-role';
import {
  flexColumnAt,
  gapPx,
  insetsPx,
  shrinksAt,
  targetHeightPx,
  textRunPx,
} from '@/__tests__/utils/tailwind-box';

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

  // Lane cd-fixes-controls (review-fix-logic-pages NOTE 4): a select whose label does not say
  // which choice it is (a tab row's narrow form stands after "showing") takes a name of its own,
  // and the panel its choice shows; the label stays the word it stands after
  it('takes a name of its own where its label does not say which choice it is, and its panel', () => {
    render(
      <Select
        label="showing"
        name="Transaction views"
        controls="views-panel"
        value="holders"
        options={SORTS}
        onChange={() => {}}
      />
    );
    const select = screen.getByRole<HTMLSelectElement>('combobox', { name: 'Transaction views' });
    expect([...(select.labels ?? [])].map((l) => l.textContent)).toEqual(['showing']);
    expect(select).toHaveAttribute('aria-controls', 'views-panel');
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

  it('refuses to stand without its label', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() =>
      render(<Select label=" " value="capacity" options={SORTS} onChange={() => {}} />)
    ).toThrow('Select: a select stands after its label, and this one has none');
  });
});

/**
 * Lane c2-finish, item 2: a select never cuts its value. A native select is as wide as its widest
 * option and clips the chosen one, with no mark, where its well is narrower; so the well never
 * shrinks, the pair stands on one line where it fits the line it stands in and the well goes under
 * its label where it does not (lib/control-row-fit.ts `selectWellUnderLabel`), and an option
 * wider than a 360 px window's line even alone throws.
 */
describe('Select: never a cut value', () => {
  const VERSION = [
    { value: 'a', label: '1 · #15,064,087' },
    { value: 'b', label: '12 · #21,016,901 · current' },
  ] as const;
  const Version = ({ label = 'version' }: { label?: string }) => (
    <div data-testid="line">
      <Select label={label} value="b" options={VERSION} onChange={() => {}} />
    </div>
  );
  const pairOf = () => {
    const select = screen.getByRole('combobox');
    const well = select.parentElement!;
    return { select, well, root: well.parentElement!, label: screen.getByText(/version$/) };
  };
  afterEach(() => vi.unstubAllGlobals());

  it('keeps its well whole at every width: it never shrinks below its widest option', () => {
    render(<Version />);
    const { select, well } = pairOf();
    for (const w of [1280, 640, 390, 360]) {
      expect(shrinksAt(well, w)).toBe(false);
      expect(shrinksAt(select, w)).toBe(false);
    }
    expect(select).not.toHaveClass('min-w-0');
  });

  it('draws the pair the fit measures: label, gap, frame, padding and the widest option', () => {
    render(<Version />);
    const { select, well, root, label } = pairOf();
    for (const w of [1280, 390]) {
      const drawn =
        textRunPx(label.textContent!, label, w) +
        gapPx(root, w) +
        insetsPx(well, w).box +
        insetsPx(select, w).box +
        textRunPx('12 · #21,016,901 · current', select, w);
      expect(Math.round(drawn * 100) / 100).toBe(
        selectPairPx(
          'version',
          VERSION.map((o) => o.label)
        ).pair
      );
    }
  });

  it('stands on one line where its pair fits the line it stands in', () => {
    // version: the pair 304.4 px in a 360 px window's 328
    stubChartWidth(328);
    render(<Version />);
    const { root } = pairOf();
    expect(root).toHaveAttribute('data-well', 'after');
    expect(flexColumnAt(root, 360)).toBe(false);
  });

  it('puts its well under its label where the pair is wider than its line', () => {
    // cell version: the pair 343.4 px, past 328, so the well goes under the label; in 358 it stays
    stubChartWidth(328);
    const { unmount } = render(<Version label="cell version" />);
    expect(pairOf().root).toHaveAttribute('data-well', 'under');
    expect(flexColumnAt(pairOf().root, 360)).toBe(true);
    unmount();
    stubChartWidth(358);
    render(<Version label="cell version" />);
    expect(pairOf().root).toHaveAttribute('data-well', 'after');
  });

  it('throws for an option wider than the narrowest line even alone', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    const over = 'x'.repeat(38);
    expect(() =>
      render(
        <Select
          label="state"
          value="a"
          options={[
            { value: 'a', label: 'all' },
            { value: 'b', label: over },
          ]}
          onChange={() => {}}
        />
      )
    ).toThrow(
      `select "state": its option "${over}" makes a well 333.4 px wide, wider than the 328 px line of a 360 px window`
    );
  });
});
