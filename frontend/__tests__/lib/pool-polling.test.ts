import { describe, expect, it } from 'vitest';
import { POOL_POLL_INTERVAL_MS, poolRefetchInterval } from '@/lib/pool-polling';

describe('poolRefetchInterval', () => {
  it('polls page one of the tab the user is looking at', () => {
    expect(poolRefetchInterval({ isPageOne: true, isActiveTab: true })).toBe(POOL_POLL_INTERVAL_MS);
  });

  it('does not poll a cursor page — pool rows never appear there', () => {
    expect(poolRefetchInterval({ isPageOne: false, isActiveTab: true })).toBe(false);
  });

  it('does not poll a tab nobody is looking at', () => {
    expect(poolRefetchInterval({ isPageOne: true, isActiveTab: false })).toBe(false);
  });
});
