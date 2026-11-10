import { describe, expect, it } from 'vitest';
import type { NetworkStats } from '@/lib/api';
import { chainMode } from '@/lib/chain-mode';

function stats(isSyncing: boolean): NetworkStats {
  return { syncStatus: { isSyncing } } as NetworkStats;
}

// The status line's mode (rubbing × Song print, phase 2), read from the vitals strip's own query
// and its Live mark: no second source.
describe('chainMode', () => {
  it('is unknown until the vitals arrive, and claims nothing', () => {
    expect(
      chainMode({ stats: undefined, failed: false, socketHeld: false, connected: false })
    ).toBe(null);
    expect(chainMode({ stats: undefined, failed: false, socketHeld: true, connected: false })).toBe(
      null
    );
  });

  it('is LIVE when the vitals are read and the indexer is at the tip', () => {
    expect(
      chainMode({ stats: stats(false), failed: false, socketHeld: false, connected: false })
    ).toBe('LIVE');
    // on a page holding the realtime socket, only while it is connected
    expect(
      chainMode({ stats: stats(false), failed: false, socketHeld: true, connected: true })
    ).toBe('LIVE');
  });

  it('is SYNCING while the indexer is behind the node', () => {
    expect(
      chainMode({ stats: stats(true), failed: false, socketHeld: false, connected: false })
    ).toBe('SYNCING');
  });

  it('is OFFLINE when the vitals fail, over any data they had, or the held socket is down', () => {
    expect(chainMode({ stats: undefined, failed: true, socketHeld: false, connected: false })).toBe(
      'OFFLINE'
    );
    expect(
      chainMode({ stats: stats(false), failed: true, socketHeld: true, connected: true })
    ).toBe('OFFLINE');
    expect(
      chainMode({ stats: stats(true), failed: false, socketHeld: true, connected: false })
    ).toBe('OFFLINE');
  });
});
