import { describe, it, expect } from 'vitest';
import { ApiRequestError, isNetworkInitializingError } from '@/lib/api';

describe('isNetworkInitializingError', () => {
  it('matches a 503 initializing error', () => {
    const err = new ApiRequestError(
      503,
      'initializing',
      'This network has not started syncing yet'
    );
    expect(isNetworkInitializingError(err)).toBe(true);
  });
  it('rejects other errors', () => {
    expect(isNetworkInitializingError(new ApiRequestError(503, 'warmup_pending', ''))).toBe(false);
    expect(isNetworkInitializingError(new ApiRequestError(500, 'initializing', ''))).toBe(false);
    expect(isNetworkInitializingError(null)).toBe(false);
  });
});
