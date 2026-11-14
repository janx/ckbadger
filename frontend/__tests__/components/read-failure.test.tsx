import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { ReadFailure } from '@/components/read-failure';
import { ApiRequestError } from '@/lib/api';

// U-06: one wording for an entity page's failed read, in place of five ("Could not load …",
// "Unable to read …", "Failed to load …", "… could not be loaded: ApiRequestError: …").
describe('ReadFailure', () => {
  it('says what could not be read and the API’s own words, as an alert', () => {
    render(
      <ReadFailure
        what="collection"
        error={new ApiRequestError(500, 'internal_error', 'store read failed at key 0x01')}
      />
    );
    const line = screen.getByRole('alert');
    expect(line).toHaveTextContent(
      /^Could not read this collection: API error: 500 - store read failed at key 0x01$/
    );
  });

  it('never prints an exception’s class name, and a gateway failure by its status', () => {
    const { rerender } = render(
      <ReadFailure what="block" error={new TypeError('Failed to fetch')} />
    );
    expect(screen.getByRole('alert')).toHaveTextContent(
      /^Could not read this block: Failed to fetch$/
    );
    rerender(
      <ReadFailure
        what="block"
        error={new ApiRequestError(502, 'bad_gateway', 'http://10.0.0.2')}
      />
    );
    expect(screen.getByRole('alert')).toHaveTextContent(
      /^Could not read this block: HTTP 502 · the API could not be reached$/
    );
    expect(screen.getByRole('alert')).not.toHaveTextContent(
      /TypeError|ApiRequestError|10\.0\.0\.2/
    );
  });
});
