import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { DotCellParty } from '@/components/identity/dotcell-party';
import { truncateHash } from '@/lib/utils';

/** Real testnet owner prefix of `support.cell`, and what it resolves to. */
const PREFIX = '0x57d926a44d83fc13b21ce037b1e31f4223e3c867';
const LOCK_HASH = '0x57d926a44d83fc13b21ce037b1e31f4223e3c867cfa3f60e1324d5bfd5cd742d';
const ADDRESS = 'ckt1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq';

describe('DotCellParty', () => {
  it('links a resolved party to its address and names its lock script', () => {
    render(
      <DotCellParty
        party={{
          hashPrefix: PREFIX,
          lockHash: LOCK_HASH,
          address: ADDRESS,
          scriptName: 'SECP256K1_BLAKE160',
        }}
      />
    );
    expect(screen.getByRole('link', { name: ADDRESS })).toHaveAttribute(
      'href',
      `/mainnet/address/${ADDRESS}`
    );
    expect(screen.getByText('SECP256K1_BLAKE160')).toBeInTheDocument();
    expect(screen.queryByText('unresolved')).not.toBeInTheDocument();
  });

  it('links a lock that encodes to no address by its full lock hash', () => {
    render(
      <DotCellParty
        party={{ hashPrefix: PREFIX, lockHash: LOCK_HASH, address: null, scriptName: null }}
      />
    );
    expect(screen.getByRole('link')).toHaveAttribute('href', `/mainnet/address/${LOCK_HASH}`);
    expect(screen.queryByText('unresolved')).not.toBeInTheDocument();
  });

  it('shows a bare prefix as an unresolved prefix, never as a link to an address', () => {
    render(
      <DotCellParty
        party={{ hashPrefix: PREFIX, lockHash: null, address: null, scriptName: null }}
      />
    );
    expect(screen.queryByRole('link')).not.toBeInTheDocument();
    expect(screen.getByText('unresolved')).toBeInTheDocument();
    expect(screen.getByTitle(PREFIX)).toHaveTextContent(PREFIX);
  });

  it('shortens a bare prefix in list rows but keeps the whole prefix in its tooltip', () => {
    render(
      <DotCellParty
        party={{ hashPrefix: PREFIX, lockHash: null, address: null, scriptName: null }}
        truncate
      />
    );
    expect(screen.getByTitle(PREFIX)).toHaveTextContent(truncateHash(PREFIX, 12, 10));
  });

  it('carries the test id it is given', () => {
    render(
      <DotCellParty
        party={{ hashPrefix: PREFIX, lockHash: null, address: null, scriptName: null }}
        testId="dotcell-owner"
      />
    );
    expect(screen.getByTestId('dotcell-owner')).toBeInTheDocument();
  });
});
