'use client';

import { cn } from '@/lib/utils';

/**
 * The well's material (the controls board, gate D0′): its ground (`base-void`), the frame line
 * and its inset, square. The one material of every field: the text field and the select draw
 * their box in it (`FIELD_WELL`).
 */
export const WELL_MATERIAL =
  'bg-base-void border-base-border border shadow-[inset_0_1px_3px_var(--m-well-inset)]';

/**
 * The field's well (the controls board, gate D0′): the well's material (`WELL_MATERIAL`), 28 px
 * high from 640 and 32 below it (a thumb's box), 10 px in; the focused field's line is celadon.
 * The select draws its native control in the same well (components/ui/select.tsx).
 */
export const FIELD_WELL = `${WELL_MATERIAL} focus-within:border-jade inline-flex h-8 min-w-0 items-center px-2.5 transition-colors sm:h-7`;

interface TextFieldProps {
  /** The field's name for a screen reader (it has no visible label: the placeholder prompts). */
  label: string;
  value: string;
  onChange: (value: string) => void;
  /** Lowercase, never trailing off (`search by name`), in the quiet ink. */
  placeholder?: string;
  /** A search field: `type="search"`, opening on the 7 px cinnabar square. */
  search?: boolean;
  id?: string;
  /** Its width where it stands (`w-[200px]`, `flex-1`): the well itself is as wide as it is given. */
  className?: string;
}

/**
 * A text field (the controls board, gate D0′; UI review 2026-11-14, phase C): a real `<input>` in
 * the field's well, 13 px, the typed value in the reading ink and the placeholder quiet. A search
 * field opens on the cinnabar square every command line of the site begins with (`.search-square`,
 * the header field's). The placeholder rule of the site (lowercase, no trailing dots; the
 * placeholder-case test reads it at a call site's literal) holds here for a placeholder built at
 * run time too.
 */
export function TextField({
  label,
  value,
  onChange,
  placeholder,
  search = false,
  id,
  className,
}: TextFieldProps) {
  if (placeholder !== undefined && /[A-Z]/.test(placeholder))
    throw new Error(`TextField: the placeholder "${placeholder}" is not lowercase`);
  if (placeholder !== undefined && /(\.\.\.|…)\s*$/.test(placeholder))
    throw new Error(`TextField: the placeholder "${placeholder}" trails off`);
  return (
    <label className={cn(FIELD_WELL, 'gap-2', className)}>
      {search && <span aria-hidden="true" className="search-square" />}
      <input
        id={id}
        type={search ? 'search' : 'text'}
        aria-label={label}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={placeholder}
        className="text-small text-text placeholder:text-text-dim min-w-0 flex-1 bg-transparent font-mono focus-visible:outline-none [&::-webkit-search-cancel-button]:appearance-none"
      />
    </label>
  );
}
