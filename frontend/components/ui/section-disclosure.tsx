import type { ReactNode } from 'react';

interface SectionDisclosureProps {
  /** Its words, the summary a reader opens it by: what the section holds ("Raw data"). */
  summary: ReactNode;
  /** Drawn open (the reader folds and opens it after; the browser holds its state). */
  open?: boolean;
  children: ReactNode;
}

/**
 * A section of the evidence tier a reader opens (lane cd-fixes-controls, the lead's ruling on
 * review-fix-tests SHOULD-4: a native disclosure is a control like any other, and this is the one
 * place a `<details>` is written; the controls gate reads one anywhere else as a disclosure by
 * hand). The browser's own `<details>` under the tier's 1 px rule: its summary in the quiet ink at
 * 13 px, the reading ink under the pointer; its content at 13 px under it, scrolling sideways where
 * it is wider than its line (raw data). What components/ui/detail-layout.tsx drew for its "Raw
 * data", its first use.
 *
 * Its one form today, a section of the tier; a disclosure inside a section (a field's bytes, with
 * no rule of its own) and one whose state its page holds (`onToggle`) are forms it does not have
 * yet, for the sweep that moves the pages' disclosures onto it.
 */
export function SectionDisclosure({ summary, open, children }: SectionDisclosureProps) {
  return (
    <details open={open} className="border-row-rule border-t pt-4">
      <summary className="text-text-dim hover:text-text text-small cursor-pointer">
        {summary}
      </summary>
      <div className="text-small mt-4 min-w-0 overflow-x-auto">{children}</div>
    </details>
  );
}
