// Lane B4 second fix round (review-rules-fixes-tests SHOULD-3, the coordinator's item 5): one
// decision writes a lock's words (`lockWords` in components/ui/lock-kind.tsx), off the lock's
// script where the place has it. A read of the API's name for a lock (`.lockKind`, `.lockName`)
// anywhere else is one of three kinds, each site listed here by file and count:
//
// - it feeds the decision (the row's facts handed to `rowLock` / `LockKind`);
// - it is the decision's words already written (a tile's `lockKind`, `lockWordsText`'s output);
// - a known writer outside the decision, pinned by number in a list whose length is pinned (the
//   places this round did not reach: the item pages' prose and evidence). A new read fails here.
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { describe, expect, it } from 'vitest';

const ROOT = join(__dirname, '..', '..');
const READ = /\.(lockKind|lockName)\b/g;

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return files(path);
    return /\.(ts|tsx)$/.test(name) ? [path] : [];
  });
}

/** Each file's reads of a lock's API name, by count. */
function reads(): Record<string, number> {
  const out: Record<string, number> = {};
  for (const dir of ['app', 'components', 'lib'])
    for (const path of files(join(ROOT, dir))) {
      const rel = relative(ROOT, path);
      if (rel === 'components/ui/lock-kind.tsx') continue;
      const n = (readFileSync(path, 'utf8').match(READ) ?? []).length;
      if (n > 0) out[rel] = n;
    }
  return out;
}

const FEEDS: Record<string, number> = {
  'components/asset/asset-history.tsx': 1,
  'components/asset/collection-holders.tsx': 1,
  'components/asset/collection-objects.tsx': 1,
  'components/asset/token-holders.tsx': 1,
  'components/dao/dao-board.tsx': 1,
};
const DECIDED: Record<string, number> = {
  'components/ui/cell-tile.tsx': 2,
};
/** Known writers outside the decision: 1 cell versions, 2 item evidence, 3 m-NFT issue note,
 * 4 object hero prose, 5 identity overview, 6 identity evidence. */
const KNOWN: Record<string, number> = {
  'components/asset/cell-versions.tsx': 1,
  'components/asset/item-evidence.tsx': 1,
  'components/asset/mnft-detail.tsx': 1,
  'components/asset/object-hero.tsx': 6,
  'components/identity/identity-overview.tsx': 4,
  'components/identity/identity-evidence.tsx': 1,
};

describe('a lock’s words have one writer (lane B4)', () => {
  it('reads the API’s name for a lock only where it feeds the decision, or a known site', () => {
    expect(reads()).toEqual({ ...FEEDS, ...DECIDED, ...KNOWN });
  });

  it('pins the known writers outside the decision: six files, 14 reads', () => {
    expect(Object.keys(KNOWN)).toHaveLength(6);
    expect(Object.values(KNOWN).reduce((a, b) => a + b, 0)).toBe(14);
  });
});
