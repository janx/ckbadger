// Resync fix round 3, tests NOTE-7: the party tag bits the page reads are the store's. The indexer
// writes TAG_TYPE_CALL and TAG_INPUT (the senders bit) in crates/ckbadger-store/src/types.rs; the
// page reads them from lib/api.ts, and the test checker (__tests__/utils/activity-read.ts) states
// fixtures with the page's. A bit moved in the store and not in the page fails here, where the
// API's layout pins would be regenerated and both frontend copies stay green.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { TAG_INPUT, TAG_TYPE_CALL } from '@/lib/api';

const TYPES_RS = resolve(__dirname, '../../../crates/ckbadger-store/src/types.rs');

/** The bit `name` the store states, as `pub const NAME: u16 = 1 << n;`. */
function storeBit(name: string): number {
  const source = readFileSync(TYPES_RS, 'utf8');
  const stated = new RegExp(`pub const ${name}: u16 = 1 << (\\d+);`).exec(source);
  if (stated === null) throw new Error(`${name} is not stated as \`1 << n\` in ${TYPES_RS}`);
  return 1 << Number(stated[1]);
}

describe('the party tag bits the page reads are the store’s', () => {
  it('reads TAG_INPUT, the senders bit, as the indexer writes it', () => {
    expect(TAG_INPUT).toBe(storeBit('TAG_INPUT'));
  });

  it('reads TAG_TYPE_CALL as the indexer writes it', () => {
    expect(TAG_TYPE_CALL).toBe(storeBit('TAG_TYPE_CALL'));
  });
});
