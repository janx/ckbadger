import { describe, expect, it, vi } from 'vitest';
import type { ReactNode } from 'react';
import { render, screen } from '@testing-library/react';
import { PartReadBoundary, ReadFailure } from '@/components/read-failure';
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

// Lane B4 third round (review-rules-fixes2 NOTE 2 and NOTE 3, the coordinator's item 4): a part's
// boundary says what failed and where, in the error's own words (its name where it is not a plain
// Error: a render bug is not a read failure with no key), and starts again when what it draws
// changes (a new page, a new cursor, a new address).
describe('PartReadBoundary', () => {
  const Boundary = PartReadBoundary as unknown as (props: {
    what: string;
    where: string;
    children: ReactNode;
  }) => ReactNode;
  function Throws({ error }: { error: Error | null }) {
    if (error) throw error;
    return <p>drawn</p>;
  }

  it('says what failed and where, in the error’s own words', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    render(
      <Boundary what="the holders" where="token 0xabc, top 10">
        <Throws error={new TypeError("Cannot read properties of undefined (reading 'x')")} />
      </Boundary>
    );
    expect(screen.getByRole('alert')).toHaveTextContent(
      /^Could not draw the holders \(token 0xabc, top 10\): TypeError: Cannot read properties of undefined \(reading 'x'\)$/
    );
    vi.restoreAllMocks();
  });

  it('starts again when what it draws changes', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    const { rerender } = render(
      <Boundary what="the holders" where="page 1">
        <Throws error={new Error('address ckb1… spells another lock')} />
      </Boundary>
    );
    expect(screen.getByRole('alert')).toHaveTextContent(
      /^Could not draw the holders \(page 1\): address ckb1… spells another lock$/
    );
    rerender(
      <Boundary what="the holders" where="page 2">
        <Throws error={null} />
      </Boundary>
    );
    expect(screen.getByText('drawn')).toBeInTheDocument();
    expect(screen.queryByRole('alert')).toBeNull();
    vi.restoreAllMocks();
  });
});
