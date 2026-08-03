import { describe, it, expect } from 'vitest';
import { QueryClient } from '@tanstack/react-query';
import {
  createAppQueryClient,
  getNetworkInitializingQueryMessage,
  NETWORK_INITIALIZING_BANNER_TEXT,
  NETWORK_INITIALIZING_MAX_RETRY_DELAY_MS,
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

  it('backs off exponentially while a network is initializing, capped at 30s', () => {
    const qc = createAppQueryClient({ warmupRetryDelayMs: 2000 });
    const retryDelay = qc.getDefaultOptions().queries!.retryDelay as (
      n: number,
      e: unknown
    ) => number;
    const initErr = new ApiRequestError(503, 'initializing', 'not synced yet');

    // A network can stay initializing for HOURS (sequential bulk sync), so the
    // indefinite retry must not keep every mounted query polling at 0.5 Hz.
    expect(retryDelay(0, initErr)).toBe(2000);
    expect(retryDelay(1, initErr)).toBe(4000);
    expect(retryDelay(2, initErr)).toBe(8000);
    expect(retryDelay(3, initErr)).toBe(16000);
    expect(retryDelay(4, initErr)).toBe(NETWORK_INITIALIZING_MAX_RETRY_DELAY_MS);
    expect(retryDelay(50, initErr)).toBe(NETWORK_INITIALIZING_MAX_RETRY_DELAY_MS);
    expect(NETWORK_INITIALIZING_MAX_RETRY_DELAY_MS).toBe(30_000);
  });

  it('keeps warmup_pending on a flat retry delay', () => {
    const qc = createAppQueryClient({ warmupRetryDelayMs: 2000 });
    const retryDelay = qc.getDefaultOptions().queries!.retryDelay as (
      n: number,
      e: unknown
    ) => number;
    const warmupErr = new ApiRequestError(503, 'warmup_pending', '');

    // Warmup is bounded and short-lived — it must NOT inherit the backoff.
    expect(retryDelay(0, warmupErr)).toBe(2000);
    expect(retryDelay(3, warmupErr)).toBe(2000);
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
