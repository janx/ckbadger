import { NoBreak } from '@/components/ui/no-break';
import { traitTextParts } from '@/lib/dob-genes';

/**
 * The one rendering point for trait text: a trait value, or a sentence built from trait values,
 * printed where a page prints it, every shortened id inside it one unit (NoBreak) and the text
 * around it as it is, free to wrap between its words, and third-party text to break anywhere where
 * its element says so (B3 round 2: "inscription 020172…cb69i0" broke after its ellipsis on a phone).
 * Nothing is added to the text: a copied value is the plain text. `__tests__/lib/trait-values.test.tsx`
 * refuses trait text printed anywhere else.
 */
export function TraitText({ text }: { text: string }) {
  return (
    <>
      {traitTextParts(text).map((part, i) =>
        part.unit ? <NoBreak key={i}>{part.text}</NoBreak> : part.text
      )}
    </>
  );
}
