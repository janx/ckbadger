import { describe, expect, it, vi } from 'vitest';
import { fireEvent, screen } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { DotCellParty } from '@/components/identity/dotcell-party';

/** Real testnet owner prefix of `support.cell`, and what it resolves to. */
const PREFIX = '0x57d926a44d83fc13b21ce037b1e31f4223e3c867';
const LOCK_HASH = '0x57d926a44d83fc13b21ce037b1e31f4223e3c867cfa3f60e1324d5bfd5cd742d';
const ADDRESS = 'ckt1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq';

describe('DotCellParty', () => {
  // Re-pinned on purpose (lane P2's third round, item 12; renamed, it was "… and names its lock
  // script"): the chip names the party by its address; no caller gave it a lock name, and a lock's
  // words are the one decision's, said where the page names the party's lock (a fact's note, a
  // Lock column), not a badge of the API's name beside the chip
  it('links a resolved party to its address', () => {
    render(<DotCellParty party={{ hashPrefix: PREFIX, lockHash: LOCK_HASH, address: ADDRESS }} />);
    expect(screen.getByRole('link', { name: ADDRESS })).toHaveAttribute(
      'href',
      `/mainnet/address/${ADDRESS}`
    );
    expect(screen.queryByText('SECP256K1_BLAKE160')).toBeNull();
    expect(screen.queryByText('unresolved')).not.toBeInTheDocument();
  });

  it('links a lock that encodes to no address by its full lock hash', () => {
    render(<DotCellParty party={{ hashPrefix: PREFIX, lockHash: LOCK_HASH, address: null }} />);
    expect(screen.getByRole('link')).toHaveAttribute('href', `/mainnet/address/${LOCK_HASH}`);
    expect(screen.queryByText('unresolved')).not.toBeInTheDocument();
  });

  // Lane c2-finish, rulings round 1: an id inside a link is the link, not a copy target. The lock
  // hash copied on a click while its link went to the address, so the copy was never seen; it is
  // the link alone now, its whole value in its title.
  for (const size of [undefined, 'row'] as const)
    it(`copies nothing on a click of the linked lock hash${size ? ' in a row' : ''}`, () => {
      const writeText = vi.fn().mockResolvedValue(undefined);
      Object.assign(navigator, { clipboard: { writeText } });
      try {
        render(
          <DotCellParty
            party={{ hashPrefix: PREFIX, lockHash: LOCK_HASH, address: null }}
            size={size}
          />
        );
        const link = screen.getByRole('link');
        const id = link.firstElementChild as HTMLElement;
        expect(id).toHaveAttribute('title', LOCK_HASH);
        expect(id.className).not.toMatch(/cursor-pointer/);
        fireEvent.click(id);
        expect(writeText).not.toHaveBeenCalled();
      } finally {
        Object.assign(navigator, { clipboard: undefined });
      }
    });

  it('shows a bare prefix as an unresolved prefix, never as a link to an address', () => {
    render(<DotCellParty party={{ hashPrefix: PREFIX, lockHash: null, address: null }} />);
    expect(screen.queryByRole('link')).not.toBeInTheDocument();
    expect(screen.getByText('unresolved')).toBeInTheDocument();
    expect(screen.getByTitle(PREFIX)).toHaveTextContent(PREFIX);
  });

  it('shortens a bare prefix in list rows but keeps the whole prefix in its tooltip', () => {
    render(
      <DotCellParty party={{ hashPrefix: PREFIX, lockHash: null, address: null }} size="row" />
    );
    // U-09 (re-pinned): a list row's size, 10…6 (it was 12…10)
    expect(screen.getByTitle(PREFIX)).toHaveTextContent(/^0x57d926a4…e3c867$/);
  });

  it('carries the test id it is given', () => {
    render(
      <DotCellParty
        party={{ hashPrefix: PREFIX, lockHash: null, address: null }}
        testId="dotcell-owner"
      />
    );
    expect(screen.getByTestId('dotcell-owner')).toBeInTheDocument();
  });
});
