import { describe, expect, it } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { SectionDisclosure } from '@/components/ui/section-disclosure';
import { inkOf, QUIET } from '@/__tests__/utils/ink-role';

/**
 * The section disclosure of the evidence tier (lane cd-fixes-controls, the lead's ruling on
 * review-fix-tests SHOULD-4: a native disclosure is a control like any other, and one library
 * component draws it): the browser's own `<details>` under the tier's 1 px rule, its summary in the
 * quiet ink at 13 px, its content at 13 px under it, scrolling sideways where it is wider than its
 * line. What components/ui/detail-layout.tsx drew for its "Raw data", now the one place a
 * `<details>` is written.
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

  it('stands under the tier’s rule, its summary quiet at 13 px, its content 13 px under it', () => {
    const { container } = render(
      <SectionDisclosure summary="Raw data">
        <p>the fields</p>
      </SectionDisclosure>
    );
    expect(container.querySelector('details')).toHaveClass('border-row-rule', 'border-t', 'pt-4');
    const summary = screen.getByText('Raw data');
    expect(inkOf(summary)).toBe(QUIET);
    expect(summary).toHaveClass('text-small', 'hover:text-text', 'cursor-pointer');
    const content = screen.getByText('the fields').parentElement!;
    expect(content).toHaveClass('text-small', 'mt-4', 'min-w-0', 'overflow-x-auto');
  });
});
