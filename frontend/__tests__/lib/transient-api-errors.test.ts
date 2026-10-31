// Round 3 lane F: the transient 503 kinds (contracts.md S `store_catching_up`, L0 `derive_pending`).
import { describe, expect, it } from 'vitest';
import { http, HttpResponse } from 'msw';
import { server } from '@/__tests__/msw/server';
import {
  api,
  ApiRequestError,
  type DeriveProgress,
  isDerivePendingError,
  isStoreCatchingUpError,
  isTransientApiError,
} from '@/lib/api';
import {
  createAppQueryClient,
  NETWORK_INITIALIZING_MAX_RETRY_DELAY_MS,
  retryTransientOnly,
} from '@/lib/query-client';

const catchingUp = {
  error: 'store_catching_up',
  message:
    "the API's secondary stores have not completed their first catch-up with the indexer yet",
};
const deriving = {
  error: 'derive_pending',
  message:
    'dao_live_agg is being derived by the indexer (state=running, 12000 source rows folded); retry later',
  derive: {
    aggregate: 'dao_live_agg',
    state: 'running',
    rowsDone: 12000,
    derivedFromTip: 20912345,
  } satisfies DeriveProgress,
};

describe('store_catching_up (contracts S)', () => {
  it('is read off the wire as its own transient kind', async () => {
    server.use(
      http.get('/api/:network/v1/statistics/network', () =>
        HttpResponse.json(catchingUp, { status: 503 })
      )
    );
    const error = await api.getNetworkStats().catch((e: unknown) => e);
    expect(isStoreCatchingUpError(error)).toBe(true);
    expect(isTransientApiError(error)).toBe(true);
    expect(error).toMatchObject({ status: 503, apiMessage: catchingUp.message });
  });

  it('is retried without a cap, backing off to the initializing ceiling', () => {
    const qc = createAppQueryClient({ warmupRetryLimit: 3, warmupRetryDelayMs: 2000 });
    const { retry, retryDelay } = qc.getDefaultOptions().queries! as {
      retry: (n: number, e: unknown) => boolean;
      retryDelay: (n: number, e: unknown) => number;
    };
    const err = new ApiRequestError(503, 'store_catching_up', 'domain store catch-up failed');
    expect(retry(5, err)).toBe(true);
    expect(retry(10_000, err)).toBe(true);
    expect(retryDelay(0, err)).toBe(2000);
    expect(retryDelay(1, err)).toBe(4000);
    expect(retryDelay(50, err)).toBe(NETWORK_INITIALIZING_MAX_RETRY_DELAY_MS);
  });
});

describe('derive_pending (contracts L0)', () => {
  it('keeps the derivation progress the API sends', async () => {
    server.use(
      http.get('/api/:network/v1/statistics/network', () =>
        HttpResponse.json(deriving, { status: 503 })
      )
    );
    const error = await api.getNetworkStats().catch((e: unknown) => e);
    expect(isDerivePendingError(error)).toBe(true);
    expect(isTransientApiError(error)).toBe(true);
    expect((error as ApiRequestError).derive).toEqual(deriving.derive);
  });

  it('is retried without a cap on the flat warmup delay', () => {
    const qc = createAppQueryClient({ warmupRetryLimit: 3, warmupRetryDelayMs: 2000 });
    const { retry, retryDelay } = qc.getDefaultOptions().queries! as {
      retry: (n: number, e: unknown) => boolean;
      retryDelay: (n: number, e: unknown) => number;
    };
    const err = new ApiRequestError(503, 'derive_pending', '', deriving.derive);
    expect(retry(10_000, err)).toBe(true);
    expect(retryDelay(0, err)).toBe(2000);
    expect(retryDelay(30, err)).toBe(2000);
  });

  it('is not a derive_pending without the 503 status', () => {
    expect(isDerivePendingError(new ApiRequestError(500, 'derive_pending', ''))).toBe(false);
  });
});

describe('retryTransientOnly (queries that otherwise fail at once)', () => {
  it('retries the transient 503 kinds and nothing else', () => {
    expect(retryTransientOnly(0, new ApiRequestError(503, 'store_catching_up', ''))).toBe(true);
    expect(retryTransientOnly(0, new ApiRequestError(503, 'derive_pending', ''))).toBe(true);
    expect(retryTransientOnly(0, new ApiRequestError(503, 'initializing', ''))).toBe(true);
    expect(retryTransientOnly(0, new ApiRequestError(503, 'warmup_pending', ''))).toBe(true);
    expect(retryTransientOnly(0, new ApiRequestError(404, 'not_found', ''))).toBe(false);
    expect(retryTransientOnly(0, new ApiRequestError(500, 'internal_error', ''))).toBe(false);
    expect(retryTransientOnly(0, new ApiRequestError(400, 'bad_request', ''))).toBe(false);
    expect(retryTransientOnly(0, new Error('boom'))).toBe(false);
  });
});
