import { describe, it, expect } from 'vitest';
import { QueryClient } from '@tanstack/react-query';
import {
  createAppQueryClient,
  getNetworkInitializingQueryMessage,
  NETWORK_INITIALIZING_BANNER_TEXT,
} from '@/lib/query-client';
import { ApiRequestError } from '@/lib/api';

describe('query-client retry: network-initializing vs warmup', () => {
  it('retries a network-initializing error indefinitely (past the warmup cap)', () => {
    const qc = createAppQueryClient({ warmupRetryLimit: 3 });
    const retry = qc.getDefaultOptions().queries!.retry as (n: number, e: unknown) => boolean;
    const initErr = new ApiRequestError(503, 'initializing', 'not synced yet');
    const warmupErr = new ApiRequestError(503, 'warmup_pending', '');

    // Warmup is BOUNDED: stops retrying past the configured limit (3).
    expect(retry(5, warmupErr)).toBe(false);
    // Initializing is UNBOUNDED: keeps retrying far past the warmup cap.
    expect(retry(5, initErr)).toBe(true);
    expect(retry(10_000, initErr)).toBe(true);
  });
});

describe('getNetworkInitializingQueryMessage default fallback', () => {
  it('returns the default banner text when the initializing error has an empty apiMessage', async () => {
    const qc = new QueryClient();
    const initErr = new ApiRequestError(503, 'initializing', '');
    // Drive a query to error state with an empty-message initializing error so the
    // cache holds it; retry:false makes it settle immediately (avoids the unbounded
    // initializing retry that createAppQueryClient would otherwise apply).
    await qc
      .fetchQuery({
        queryKey: ['query-client-initializing-empty-msg'],
        queryFn: () => Promise.reject(initErr),
        retry: false,
      })
      .catch(() => {});
    expect(getNetworkInitializingQueryMessage(qc)).toBe(NETWORK_INITIALIZING_BANNER_TEXT);
  });
});
