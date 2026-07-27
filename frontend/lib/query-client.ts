import { QueryClient } from '@tanstack/react-query';
import { isWarmupPendingError, isNetworkInitializingError } from '@/lib/api';

export const WARMUP_PENDING_RETRY_DELAY_MS = 2000;
/**
 * Ceiling for the network-initializing backoff. The retry itself stays
 * indefinite (the banner depends on it), but a network can sit initializing for
 * HOURS while other networks bulk-sync — polling every mounted query at the flat
 * warmup rate for that long is pure waste.
 */
export const NETWORK_INITIALIZING_MAX_RETRY_DELAY_MS = 30_000;
const DEFAULT_WARMUP_RETRY_LIMIT = 120;
export const WARMUP_PENDING_BANNER_TEXT = 'Data is being prepared. Retrying automatically...';
export const NETWORK_INITIALIZING_BANNER_TEXT =
  'This network has not started syncing yet — it will begin after the other networks finish their initial sync.';

interface CreateAppQueryClientOptions {
  gcTime?: number;
  staleTime?: number;
  refetchOnWindowFocus?: boolean;
  nonWarmupRetry?: boolean | number;
  warmupRetryLimit?: number;
  warmupRetryDelayMs?: number;
}

function shouldRetryNonWarmup(
  failureCount: number,
  nonWarmupRetry: boolean | number | undefined
): boolean {
  if (typeof nonWarmupRetry === 'number') {
    return failureCount < nonWarmupRetry;
  }
  if (typeof nonWarmupRetry === 'boolean') {
    return nonWarmupRetry;
  }
  return failureCount < 2;
}

export function createAppQueryClient(options: CreateAppQueryClientOptions = {}): QueryClient {
  const warmupRetryLimit = options.warmupRetryLimit ?? DEFAULT_WARMUP_RETRY_LIMIT;
  const warmupRetryDelayMs = options.warmupRetryDelayMs ?? WARMUP_PENDING_RETRY_DELAY_MS;

  return new QueryClient({
    defaultOptions: {
      queries: {
        staleTime: options.staleTime ?? 10 * 1000,
        gcTime: options.gcTime ?? 5 * 60 * 1000,
        refetchOnWindowFocus: options.refetchOnWindowFocus ?? false,
        retry: (failureCount, error) => {
          if (isNetworkInitializingError(error)) {
            return true; // keep retrying: the network will start when its turn comes
          }
          if (isWarmupPendingError(error)) {
            return failureCount < warmupRetryLimit;
          }
          return shouldRetryNonWarmup(failureCount, options.nonWarmupRetry);
        },
        retryDelay: (attemptIndex, error) => {
          if (isNetworkInitializingError(error)) {
            return Math.min(
              warmupRetryDelayMs * 2 ** attemptIndex,
              NETWORK_INITIALIZING_MAX_RETRY_DELAY_MS
            );
          }
          if (isWarmupPendingError(error)) {
            return warmupRetryDelayMs;
          }
          return Math.min(1000 * (attemptIndex + 1), 3000);
        },
      },
      mutations: {
        retry: false,
      },
    },
  });
}

function warmupMessageFromError(error: unknown): string | null {
  if (!isWarmupPendingError(error)) {
    return null;
  }
  const apiMessage =
    typeof (error as { apiMessage?: unknown }).apiMessage === 'string'
      ? (error as { apiMessage: string }).apiMessage
      : '';
  return apiMessage || WARMUP_PENDING_BANNER_TEXT;
}

export function getWarmupPendingQueryMessage(queryClient: QueryClient): string | null {
  for (const query of queryClient.getQueryCache().getAll()) {
    const fetchFailureReason = (query.state as { fetchFailureReason?: unknown }).fetchFailureReason;
    const candidates = [fetchFailureReason, query.state.error];
    for (const candidate of candidates) {
      const message = warmupMessageFromError(candidate);
      if (message) {
        return message;
      }
    }
  }
  return null;
}

export function getNetworkInitializingQueryMessage(queryClient: QueryClient): string | null {
  for (const query of queryClient.getQueryCache().getAll()) {
    const fetchFailureReason = (query.state as { fetchFailureReason?: unknown }).fetchFailureReason;
    for (const candidate of [fetchFailureReason, query.state.error]) {
      if (isNetworkInitializingError(candidate)) {
        const apiMessage =
          typeof (candidate as { apiMessage?: unknown }).apiMessage === 'string'
            ? (candidate as { apiMessage: string }).apiMessage
            : '';
        return apiMessage || NETWORK_INITIALIZING_BANNER_TEXT;
      }
    }
  }
  return null;
}
