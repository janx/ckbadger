// Kept apart from `@/lib/api` so the query client's retry rule does not depend on how a test
// mocks the API module.
import type { ApiRequestError } from '@/lib/api';

/**
 * "The resource does not exist", read from the HTTP status the API answered
 * with — never from the rendered message text, which is prose and changes.
 */
export function isNotFoundApiError(error: unknown): error is { status: 404 } {
  if (!error || typeof error !== 'object') {
    return false;
  }
  return (error as { status?: unknown }).status === 404;
}

/** A 503 whose `error` code is `code` (read from the fields, so a plain object matches too). */
function is503(error: unknown, code: string): error is ApiRequestError {
  if (!error || typeof error !== 'object') {
    return false;
  }
  const candidate = error as Partial<ApiRequestError>;
  return candidate.code === code && candidate.status === 503;
}

export function isWarmupPendingError(error: unknown): error is ApiRequestError {
  return is503(error, 'warmup_pending');
}

export function isNetworkInitializingError(error: unknown): error is ApiRequestError {
  return is503(error, 'initializing');
}

/**
 * Round 3 (contracts.md S): the API's secondary stores have not caught up with the indexer (at
 * startup, or after 3 consecutive failed catch-ups). Transient: retry behind the "catching up"
 * banner, never the error screen. It can precede `warmup_pending` on the same endpoint.
 */
export function isStoreCatchingUpError(error: unknown): error is ApiRequestError {
  return is503(error, 'store_catching_up');
}

/**
 * Round 3 (contracts.md L0): an indexer-derived aggregate is not derived yet. Transient: retry,
 * show "being derived" + the progress in `derive`, never a stand-in value.
 */
export function isDerivePendingError(error: unknown): error is ApiRequestError {
  return is503(error, 'derive_pending');
}

/** Every 503 kind that means "not yet, retry" for the whole API (one list, used by the retry policy). */
export function isTransientApiError(error: unknown): error is ApiRequestError {
  return (
    isNetworkInitializingError(error) ||
    isWarmupPendingError(error) ||
    isStoreCatchingUpError(error) ||
    isDerivePendingError(error)
  );
}
