import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import userEvent from '@testing-library/user-event';
import { render, screen } from '@testing-library/react';
import { FIELD_WELL, TextField, WELL_MATERIAL } from '@/components/ui/text-field';
import { inkOf, READING } from '@/__tests__/utils/ink-role';
import { targetHeightPx } from '@/__tests__/utils/tailwind-box';

/**
 * The text field of the controls board (gate D0′): the header search field's well (the well's
 * ground, the frame line, the inset), 28 px high and 32 below 640, 13 px; a search field opens on
 * the 7 px cinnabar square; a placeholder is lowercase and quiet, a typed value the reading ink.
 * A real <input> inside.
 */
function Field({ search = true, placeholder = 'search by name' }) {
  const [value, setValue] = useState('');
  return (
    <TextField
      label="search the inventory"
      value={value}
      onChange={setValue}
      placeholder={placeholder}
      search={search}
    />
  );
}

afterEach(() => vi.restoreAllMocks());

describe('TextField', () => {
  it('is a real search input named for a screen reader, its placeholder its prompt', () => {
    render(<Field />);
    const input = screen.getByRole('searchbox', { name: 'search the inventory' });
    expect(input.tagName).toBe('INPUT');
    expect(input).toHaveAttribute('type', 'search');
    expect(input).toHaveAttribute('placeholder', 'search by name');
  });

  it('opens a search field on the 7 px cinnabar square, and a plain field on none', () => {
    const { container, unmount } = render(<Field />);
    const square = container.querySelector('.search-square');
    expect(square).toHaveAttribute('aria-hidden', 'true');
    expect(square!.nextElementSibling?.tagName).toBe('INPUT');
    unmount();
    const plain = render(<Field search={false} placeholder="country" />);
    expect(plain.container.querySelector('.search-square')).toBeNull();
    expect(screen.getByRole('textbox', { name: 'search the inventory' })).toHaveAttribute(
      'type',
      'text'
    );
  });

  it('types in the reading ink at 13 px, the placeholder in the quiet ink', () => {
    render(<Field />);
    const input = screen.getByRole('searchbox');
    expect(input).toHaveClass('text-small', 'placeholder:text-text-dim');
    expect(inkOf(input)).toBe(READING);
  });

  it('answers what is typed', async () => {
    const user = userEvent.setup();
    render(<Field />);
    await user.type(screen.getByRole('searchbox'), 'usdc');
    expect(screen.getByRole('searchbox')).toHaveValue('usdc');
  });

  it('is the header field’s well: the void ground, the frame line, the inset, square', () => {
    render(<Field />);
    const well = screen.getByRole('searchbox').parentElement!;
    expect(well).toHaveClass('bg-base-void', 'border', 'border-base-border');
    expect(well.className).toMatch(/shadow-\[inset_0_1px_3px_var\(--m-well-inset\)\]/);
    expect(well.className).not.toMatch(/rounded/);
  });

  // Lane C2 (topics): the board's field copies the header search field's well; the header's
  // search field and the command palette's query field are comboboxes of their own (their cursor,
  // their list, their keys), not TextFields, so the well is one constant every field reads: the
  // text field's and the select's box (FIELD_WELL) and the two comboboxes' own boxes.
  it('is one material, read by the text field, the select and the shell’s two comboboxes', () => {
    const root = join(__dirname, '..', '..');
    const source = (file: string) => readFileSync(join(root, file), 'utf8');
    expect(FIELD_WELL.split(' ')).toEqual(expect.arrayContaining(WELL_MATERIAL.split(' ')));
    expect(source('components/ui/select.tsx')).toMatch(
      /import \{ FIELD_WELL \} from '@\/components\/ui\/text-field';/
    );
    for (const file of ['components/search-bar.tsx', 'components/command-palette.tsx'])
      expect(source(file), file).toMatch(
        /import \{[^}]*\bWELL_MATERIAL\b[^}]*\} from '@\/components\/ui\/text-field';/
      );
  });

  it('stands 28 px high from 640 and 32 below it', () => {
    render(<Field />);
    const well = screen.getByRole('searchbox').parentElement!;
    expect([1280, 640, 639, 390, 360].map((w) => targetHeightPx(well, w))).toEqual([
      28, 28, 32, 32, 32,
    ]);
  });

  it('refuses a placeholder that is not lowercase, or that trails off', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() => render(<Field placeholder="Search by name" />)).toThrow(
      'TextField: the placeholder "Search by name" is not lowercase'
    );
    expect(() => render(<Field placeholder="search by name…" />)).toThrow('trails off');
  });
});
