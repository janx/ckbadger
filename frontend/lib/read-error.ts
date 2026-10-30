import { ApiRequestError } from '@/lib/api';

/**
 * A read failure as a page states it (ID-COL-1). The frontend server's gateway errors (502 /
 * 503 / 504) carry its own proxy message, which names an internal URL and says nothing about
 * the chain; the status says what happened. Every other error keeps its message, which is the
 * API's own context (block, tx, key) for finding the cause.
 */
const GATEWAY: Record<number, string> = {
  502: 'the API could not be reached',
  503: 'the API is not available',
  504: 'the API did not answer in time',
};

export function readErrorText(error: Error): string {
  if (error instanceof ApiRequestError && GATEWAY[error.status])
    return `HTTP ${error.status} · ${GATEWAY[error.status]}`;
  return error.message;
}
