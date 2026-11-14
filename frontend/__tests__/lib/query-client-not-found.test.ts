import { describe, expect, it } from 'vitest';
import { createAppQueryClient } from '@/lib/query-client';
import { ApiRequestError } from '@/lib/api';

describe('query-client retry: a definite 404 (FF N4)', () => {
  it('does not retry a resource the API says does not exist', () => {
    const qc = createAppQueryClient();
    const retry = qc.getDefaultOptions().queries!.retry as (n: number, e: unknown) => boolean;
    expect(retry(0, new ApiRequestError(404, 'not_found', 'no such fork event'))).toBe(false);
  });

  // U-06 / R11: a 400 says the request names nothing that could exist; asking again gets the
  // same answer, after a skeleton (the malformed-address page waited about 3.4 s for two retries).
  it('does not retry a request the API refuses as malformed (400)', () => {
    const qc = createAppQueryClient();
    const retry = qc.getDefaultOptions().queries!.retry as (n: number, e: unknown) => boolean;
    expect(
      retry(0, new ApiRequestError(400, 'bad_request', 'Invalid CKB address: parse failed'))
    ).toBe(false);
  });

  it('still retries a transient server failure', () => {
    const qc = createAppQueryClient();
    const retry = qc.getDefaultOptions().queries!.retry as (n: number, e: unknown) => boolean;
    expect(retry(0, new ApiRequestError(500, 'internal', 'boom'))).toBe(true);
  });
});
