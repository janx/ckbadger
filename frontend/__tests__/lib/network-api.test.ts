import { describe, it, expect } from 'vitest';
import { api } from '@/lib/api';
// MSW handlers (Task adds them) serve /network/summary etc. from __tests__/msw/handlers.ts

describe('network api', () => {
  it('fetches summary', async () => {
    const s = await api.getNetworkSummary();
    expect(typeof s.enabled).toBe('boolean');
    expect(typeof s.hasData).toBe('boolean');
  });
  it('fetches distributions', async () => {
    const d = await api.getNetworkDistributions();
    expect(Array.isArray(d.versions)).toBe(true);
  });
});
