import type { ReactNode } from 'react';
import { useDetailTier } from '@/components/ui/detail-tier';
import { cn } from '@/lib/utils';

interface SectionDisclosureBase {
  /** Its words, the summary a reader opens it by: what it holds ("Raw data", "Decoded fields"). */
  summary: ReactNode;
  /**
   * `section` (the default): a section of its tier, under the tier's 1 px rule, its content at
   * 13 px, 16 px under its words. `inner`: a disclosure inside a section (a field's bytes, a list's
   * every row), with no rule of its own, its content its caller's, 8 px under its words.
   */
  form?: 'section' | 'inner';
  /** Its place in its caller's layout: a margin, never its look. */
  className?: string;
  children: ReactNode;
}

/**
 * Its state: the browser's (`open` draws it open, the reader folds and opens it after), or its
 * page's (lane cd-fixes-controls, the disclosure sweep): `open` with `onToggle`, for a page that
 * reads whether it is open (the transaction page's Cell graph loads only when it is open); the
 * browser still opens it, the page is told, and the page's state is what it draws.
 */
type SectionDisclosureState =
  | { open?: boolean; onToggle?: never }
  | { open: boolean; onToggle: (open: boolean) => void };

type SectionDisclosureProps = SectionDisclosureBase & SectionDisclosureState;

/**
 * The tier rule: a disclosure's words are a control's words, at the small step (13 px) in both
 * tiers, as the library's other controls set theirs (FoldToggle's words, the Select's label);
 * their ink is the tier's, the reading ink in tier 1, where everyone reads (a tab's name not
 * taken, the Select's chosen value), and the quiet ink in tier 2, whose voice is quieter (its
 * headings); under the pointer one step up. One answer per tier, the same in either form.
 */
const WORDS_INK = {
  1: 'text-text hover:text-text-bright',
  2: 'text-text-dim hover:text-text',
} as const;

/**
 * A disclosure a reader opens (lane cd-fixes-controls, the lead's ruling on review-fix-tests
 * SHOULD-4: a native disclosure is a control like any other, and this is the one place a
 * `<details>` is written; the controls gate reads one anywhere else as a disclosure by hand). The
 * browser's own `<details>`: its summary set by the tier rule above, in either form (`form`), its
 * state the browser's or its page's (`onToggle`); the section form is what
 * components/ui/detail-layout.tsx drew for its "Raw data", its first use.
 */
export function SectionDisclosure({
  summary,
  form = 'section',
  open,
  onToggle,
  className,
  children,
}: SectionDisclosureProps) {
  const tier = useDetailTier();
  const inner = form === 'inner';
  return (
    <details
      open={open}
      onToggle={onToggle && ((event) => onToggle(event.currentTarget.open))}
      className={cn('min-w-0', !inner && 'border-row-rule border-t pt-4', className)}
    >
      <summary className={cn('text-small cursor-pointer', WORDS_INK[tier])}>{summary}</summary>
      <div className={cn('min-w-0', inner ? 'mt-2' : 'text-small mt-4')}>{children}</div>
    </details>
  );
}
