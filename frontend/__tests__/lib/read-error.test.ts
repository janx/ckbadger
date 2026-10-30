import { readErrorText } from '@/lib/read-error';
import { ApiRequestError } from '@/lib/api';

describe('readErrorText (ID-COL-1)', () => {
  it('states a gateway failure by its status, never the proxy message with its internal URL', () => {
    const e = new ApiRequestError(
      504,
      'unknown_error',
      "upstream network 'mainnet' timed out: error sending request for url (http://127.0.0.1:8101/x)"
    );
    expect(readErrorText(e)).toBe('HTTP 504 · the API did not answer in time');
    expect(readErrorText(new ApiRequestError(502, 'bad_gateway', 'x'))).toBe(
      'HTTP 502 · the API could not be reached'
    );
  });
  it("keeps the API's own message, which names the block / tx to look at", () => {
    const e = new ApiRequestError(500, 'internal_error', 'bucket=4 block_num=12 tx_idx=3');
    expect(readErrorText(e)).toBe(e.message);
    expect(readErrorText(new Error('missing local cell bytes'))).toBe('missing local cell bytes');
  });
});
