import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { http, HttpResponse } from 'msw';
import { api } from '@/lib/api';
import { server } from '../msw/server';
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

// The data layer must target `/api/<active-network>/v1/*` where the active network is derived
// from the URL path. Seed a two-network runtime config, drive window.location via pushState, and
// assert the network segment the request actually lands on (captured from the MSW path param).
describe('network-scoped api routing', () => {
  beforeEach(() => {
    window.__CKBADGER_RUNTIME_CONFIG__ = {
      networks: [{ name: 'mainnet' }, { name: 'testnet' }],
      defaultNetwork: 'mainnet',
    };
  });

  afterEach(() => {
    // Restore location + clear seeded config so state does not leak into sibling test files that
    // share the same jsdom window instance.
    window.history.pushState({}, '', '/');
    delete window.__CKBADGER_RUNTIME_CONFIG__;
  });

  function captureBlocksNetwork(): () => string | undefined {
    let seen: string | undefined;
    server.use(
      http.get('/api/:network/v1/blocks', ({ params }) => {
        seen = params.network as string;
        return HttpResponse.json({
          data: [],
          total: 0,
          limit: 20,
          hasMore: false,
          nextCursor: null,
        });
      })
    );
    return () => seen;
  }

  it('targets the testnet proxy path when the URL is under /testnet', async () => {
    window.history.pushState({}, '', '/testnet/blocks');
    const seen = captureBlocksNetwork();

    await api.getBlocks();

    expect(seen()).toBe('testnet');
  });

  it('targets the mainnet proxy path when the URL is under /mainnet', async () => {
    window.history.pushState({}, '', '/mainnet/blocks');
    const seen = captureBlocksNetwork();

    await api.getBlocks();

    expect(seen()).toBe('mainnet');
  });
});
