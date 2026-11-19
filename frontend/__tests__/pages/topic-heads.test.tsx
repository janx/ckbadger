import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

// Lane D2 (U-11, PB-24): the topic pages' h1 stands at one line position. The header ends at the
// same y on every page, and nothing stands between it and a topic page's h1 but its main's top
// padding, so the h1's position is that padding: measured in the UI review, network 139 (16 px),
// DAO and hardforks 147 (24 px), forks and fiber 151 (28 px), the list pages 155 (32 px). One
// position is the list pages' and the entity pages': 32 px, `pt-8` (or `py-8`), on every topic
// page and every list page, in every state the page draws.

const PAGES = [
  // topic pages
  'app/network/client-page.tsx',
  'app/dao/page.tsx',
  'app/hardforks/page.tsx',
  'app/forks/page.tsx',
  'app/fiber/channels/client-page.tsx',
  'app/scripts/page.tsx',
  // list pages
  'app/activities/page.tsx',
  'app/blocks/page.tsx',
  'app/transactions/page.tsx',
  'app/charts/page.tsx',
  'app/inventory/inventory-page-client.tsx',
];

/** The top padding classes of every `<main>` a page writes. */
function mainTops(path: string): string[] {
  const source = readFileSync(resolve(__dirname, '../..', path), 'utf8');
  const mains = [...source.matchAll(/<main className="([^"]+)"/g)].map((m) => m[1]);
  expect(mains.length, `${path} writes a <main>`).toBeGreaterThan(0);
  return mains.map(
    (classes) =>
      classes
        .split(/\s+/)
        .filter((c) => /^(?:pt|py)-/.test(c))
        .join(' ') || '(none)'
  );
}

describe('the topic and list pages’ h1 at one line position (lane D2, U-11)', () => {
  it.each(PAGES)('%s opens its main 32 px under the header', (path) => {
    for (const top of mainTops(path))
      expect([path, top]).toEqual([path, expect.stringMatching(/^(pt-8|py-8)$/)]);
  });
});
