import type { ReactNode } from 'react';
import { cn } from '@/lib/utils';

/** One JSON token: a string (a key when a colon follows), a number, or true / false / null. */
const TOKEN =
  /("(?:\\.|[^"\\])*")(\s*:)?|(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)|\b(true|false|null)\b/g;

/** Keys in the lock ink, values in the readable grey, punctuation dim (Main board, tier 2). */
export const JSON_INKS = { key: 'var(--c-cell-lock)', value: 'var(--c-text)' } as const;

/**
 * JSON text as the boards print it in tier 2 (consistency lane E): the exact text, unchanged, with
 * its keys and values tinted so the eye finds them; the rest of the text (braces, commas) stays dim.
 * The text is passed in already formatted, so what is shown is what was given, character for
 * character.
 */
export function TintedJson({ text, className }: { text: string; className?: string }) {
  const parts: ReactNode[] = [];
  let last = 0;
  for (const m of text.matchAll(TOKEN)) {
    const at = m.index;
    if (at > last) parts.push(text.slice(last, at));
    if (m[1] !== undefined && m[2] !== undefined) {
      parts.push(
        <span key={at} data-json="key" style={{ color: JSON_INKS.key }}>
          {m[1]}
        </span>,
        m[2]
      );
    } else {
      parts.push(
        <span key={at} data-json="value" style={{ color: JSON_INKS.value }}>
          {m[0]}
        </span>
      );
    }
    last = at + m[0].length;
  }
  if (last < text.length) parts.push(text.slice(last));
  return (
    <pre
      className={cn(
        // a well (material pass v2, move 2): raw bytes are read from dark inset glass
        'well text-meta text-text-dim max-w-full whitespace-pre-wrap break-all px-3 py-2.5 leading-relaxed',
        className
      )}
    >
      {parts}
    </pre>
  );
}
