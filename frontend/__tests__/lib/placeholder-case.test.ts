import { readdirSync, readFileSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

// UI review 2026-11-14 (U-01, R2): the header's field reads as a command line, lowercase and
// without dots ("search block / tx / address / cell"), and every other search or filter field
// follows it ("search by name", "search by name or code hash", "country"). One rule, read from
// the source so a new field cannot bring back "Search by name..." or "Search names or IDs".

const root = resolve(__dirname, '../..');

function files(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
    e.isDirectory() ? files(join(dir, e.name)) : /\.tsx?$/.test(e.name) ? [join(dir, e.name)] : []
  );
}

/** Every placeholder text written in the source: `placeholder="…"`, the string literals inside
 * `placeholder={…}`, and the `…PLACEHOLDER… = '…'` constants. */
function placeholders(source: string): string[] {
  const found: string[] = [];
  for (const m of source.matchAll(/placeholder="([^"]*)"/g)) found.push(m[1]);
  for (const m of source.matchAll(/placeholder=\{([^}]*)\}/g))
    for (const s of m[1].matchAll(/'([^']*)'|"([^"]*)"|`([^`]*)`/g))
      found.push(s[1] ?? s[2] ?? s[3]);
  for (const m of source.matchAll(/\w*PLACEHOLDER\w*\s*=\s*'([^']*)'/g)) found.push(m[1]);
  return found;
}

describe('field placeholders (U-01, R2)', () => {
  // lib holds the header field's two placeholders since readability F3-f (lib/header-fit.ts: the
  // header's budget floors the field at the short one), so the rule reads it too
  const written = ['app', 'components', 'lib']
    .flatMap((dir) => files(join(root, dir)))
    .flatMap((file) =>
      placeholders(readFileSync(file, 'utf-8')).map((text) => ({
        file: relative(root, file),
        text,
      }))
    );

  it('finds the fields it rules on', () => {
    expect(written.map((p) => p.text)).toEqual(
      expect.arrayContaining([
        'search block / tx / address / cell',
        'search',
        'search by name',
        'search by name or code hash',
        'search names or ids',
        'country',
        'version',
      ])
    );
  });

  it('writes every placeholder in lowercase, with no trailing dots', () => {
    const wrong = written.filter(({ text }) => /[A-Z]/.test(text) || /(\.\.\.|…)\s*$/.test(text));
    expect(wrong).toEqual([]);
  });
});
