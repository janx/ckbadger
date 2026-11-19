import { describe, expect, it } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { SectionDisclosure } from '@/components/ui/section-disclosure';
import { DetailTier2 } from '@/components/ui/detail-layout';
import { inkOf, QUIET, READING } from '@/__tests__/utils/ink-role';

/**
 * The section disclosure of the evidence tier (lane cd-fixes-controls, the lead's ruling on
 * review-fix-tests SHOULD-4: a native disclosure is a control like any other, and one library
 * component draws it): the browser's own `<details>` under the tier's 1 px rule, its summary at
 * 13 px, its content 13 px under it. What components/ui/detail-layout.tsx drew for its "Raw data",
 * now the one place a `<details>` is written.
 */
describe('SectionDisclosure', () => {
  it('is the browser’s own disclosure: a details whose summary is its words, shut until opened', () => {
    const { container } = render(
      <SectionDisclosure summary="Raw data">
        <pre>{'{ "hash": "0x01" }'}</pre>
      </SectionDisclosure>
    );
    const details = container.querySelector('details')!;
    const summary = screen.getByText('Raw data');
    expect(summary.tagName).toBe('SUMMARY');
    expect(summary.parentElement).toBe(details);
    expect(details).not.toHaveAttribute('open');
    fireEvent.click(summary);
    expect(details).toHaveAttribute('open');
  });

  it('is drawn open where its caller opens it', () => {
    const { container } = render(
      <SectionDisclosure summary="Raw data" open>
        <p>the fields</p>
      </SectionDisclosure>
    );
    expect(container.querySelector('details')).toHaveAttribute('open');
  });

  // Re-pinned on purpose (lane cd-fixes-controls, the disclosure sweep): the summary's ink is its
  // tier's (below), so the quiet ink is read in tier 2, where "Raw data" stands; and its content no
  // longer scrolls sideways itself: a scroll container under a table's sticky head holds the head
  // to it, not to the page (the token's supply table is in one), so the raw data's caller scrolls
  // it (components/ui/detail-layout.tsx)
  it('stands under the tier’s rule, its summary at 13 px, its content 13 px and 16 px under it', () => {
    const { container } = render(
      <DetailTier2>
        <SectionDisclosure summary="Raw data">
          <p>the fields</p>
        </SectionDisclosure>
      </DetailTier2>
    );
    expect(container.querySelector('details')).toHaveClass('border-row-rule', 'border-t', 'pt-4');
    const summary = screen.getByText('Raw data');
    expect(inkOf(summary)).toBe(QUIET);
    expect(summary).toHaveClass('text-small', 'hover:text-text', 'cursor-pointer');
    const content = screen.getByText('the fields').parentElement!;
    expect(content).toHaveClass('text-small', 'mt-4', 'min-w-0');
    expect(content).not.toHaveClass('overflow-x-auto');
  });
});

/**
 * The tier rule (lane cd-fixes-controls, the disclosure sweep): a disclosure's words are a
 * control's words, at the small step, 13 px, in both tiers, as the library's other controls set
 * theirs (FoldToggle's words, the Select's label); their ink is the tier's: the reading ink in tier
 * 1, where everyone reads (a tab's name not taken, the Select's chosen value), and the quiet ink in
 * tier 2, whose voice is quieter (its headings); under the pointer one step up. One answer per
 * tier, the same in either form.
 */
describe('SectionDisclosure: its words by the tier it stands in', () => {
  it('sets its words at 13 px in both tiers: the reading ink in tier 1, the quiet ink in tier 2', () => {
    render(
      <>
        <SectionDisclosure summary="Every change to the supply (3)">
          <p>the changes</p>
        </SectionDisclosure>
        <DetailTier2>
          <SectionDisclosure summary="Raw data">
            <p>the fields</p>
          </SectionDisclosure>
        </DetailTier2>
      </>
    );
    const one = screen.getByText('Every change to the supply (3)');
    const two = screen.getByText('Raw data');
    expect(one).toHaveClass('text-small', 'hover:text-text-bright');
    expect(inkOf(one)).toBe(READING);
    expect(two).toHaveClass('text-small', 'hover:text-text');
    expect(inkOf(two)).toBe(QUIET);
  });
});

/**
 * The inner form (lane cd-fixes-controls, the disclosure sweep): a disclosure inside a section,
 * a field's bytes or a list's every row, with no rule of its own. It keeps the section form's words
 * (13 px, the tier's ink); its content is its caller's, at the caller's steps, 8 px under the words.
 */
describe('SectionDisclosure: the inner form', () => {
  it('draws no rule of its own, keeps its words by its tier, its content 8 px under them', () => {
    const { container } = render(
      <DetailTier2>
        <SectionDisclosure form="inner" summary="Decoded fields">
          <pre>{'{}'}</pre>
        </SectionDisclosure>
      </DetailTier2>
    );
    const details = container.querySelector('details')!;
    expect(details).not.toHaveClass('border-t');
    expect(details.className).not.toMatch(/\bpt-/);
    const summary = screen.getByText('Decoded fields');
    expect(summary).toHaveClass('text-small', 'cursor-pointer');
    expect(inkOf(summary)).toBe(QUIET);
    const content = screen.getByText('{}').parentElement!;
    expect(content).toHaveClass('mt-2', 'min-w-0');
    expect(content).not.toHaveClass('text-small');
    fireEvent.click(summary);
    expect(details).toHaveAttribute('open');
  });

  it('reads the reading ink in tier 1, and takes its place in its caller’s layout, a margin', () => {
    const { container } = render(
      <SectionDisclosure form="inner" summary="Media dependencies · 2 sources" className="mt-3">
        <ul>
          <li>ipfs://a</li>
        </ul>
      </SectionDisclosure>
    );
    expect(inkOf(screen.getByText('Media dependencies · 2 sources'))).toBe(READING);
    expect(container.querySelector('details')).toHaveClass('mt-3', 'min-w-0');
  });
});
