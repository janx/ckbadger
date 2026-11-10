import { describe, expect, it } from 'vitest';
import { SHORTCUT_HINTS, STATUS_KEY_HINTS } from '@/lib/shortcuts';

describe('the keyboard shortcuts', () => {
  it('gives the status line three key hints from the help’s own list, in its order', () => {
    expect(STATUS_KEY_HINTS).toEqual([
      { keys: '?', short: 'help' },
      { keys: '/', short: 'search' },
      { keys: 'g b', short: 'blocks' },
    ]);
    for (const hint of STATUS_KEY_HINTS)
      expect(SHORTCUT_HINTS.some((s) => s.keys === hint.keys && s.short === hint.short)).toBe(true);
  });

  it('keeps every key once', () => {
    const keys = SHORTCUT_HINTS.map((s) => s.keys);
    expect(new Set(keys).size).toBe(keys.length);
  });
});
