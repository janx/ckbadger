import { render, screen } from '@testing-library/react';
import { ParticipantRefView } from '@/components/participant-ref';
import type { ParticipantRef } from '@/lib/api';
import { truncateHash } from '@/lib/utils';

function makeRef(overrides: Partial<ParticipantRef> = {}): ParticipantRef {
  return {
    address: overrides.address ?? null,
    lockHash: overrides.lockHash ?? null,
    lockHashPrefix: overrides.lockHashPrefix ?? null,
    roles: overrides.roles ?? [],
  };
}

describe('ParticipantRefView', () => {
  const address =
    'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqt4z78ng4yutl5u6xsv27lt52eh9jvvtd9wj5clj';

  it('renders a linked address when the party resolved', () => {
    render(<ParticipantRefView participant={makeRef({ address, lockHash: '0xabcd' })} />);
    const link = screen.getByRole('link');
    expect(link).toHaveAttribute('href', `/mainnet/address/${address}`);
  });

  it('renders the prefix and an unresolved marker when it did not, with no link', () => {
    const prefix = '0x1e3a88ca5cc39f1bd38c091b53e33b7c29ebd019';
    render(<ParticipantRefView participant={makeRef({ lockHashPrefix: prefix })} />);
    expect(screen.queryByRole('link')).toBeNull();
    expect(screen.getByText(/unresolved/i)).toBeInTheDocument();
    // The full prefix stays reachable even though the label is shortened.
    expect(screen.getByTitle(prefix)).toBeInTheDocument();
  });

  it('shortens an unresolved prefix the way every other hash is shortened', () => {
    const prefix = '0x1e3a88ca5cc39f1bd38c091b53e33b7c29ebd019';
    render(<ParticipantRefView participant={makeRef({ lockHashPrefix: prefix })} />);
    expect(screen.getByTitle(prefix)).toHaveTextContent(truncateHash(prefix, 10, 6));
  });

  // A Lock party whose lock script the store does not know encodes to no
  // address; the API sends `address: null` with the lock hash it does know.
  const LOCK_HASH = '0x57d926a44d83fc13b21ce037b1e31f4223e3c867cfa3f60e1324d5bfd5cd742d';

  it('shows a lock party without an address as its lock hash, linked by hash', () => {
    render(<ParticipantRefView participant={makeRef({ lockHash: LOCK_HASH })} />);
    const link = screen.getByRole('link');
    expect(link).toHaveAttribute('href', `/mainnet/address/${LOCK_HASH}`);
    expect(link).toHaveAttribute('title', LOCK_HASH);
    expect(link).toHaveTextContent(truncateHash(LOCK_HASH, 10, 6));
    // It is resolved to a lock, just not to an address.
    expect(screen.queryByText(/unresolved/i)).toBeNull();
  });

  it('shows a lock party without an address as its lock hash in the compact form too', () => {
    render(<ParticipantRefView participant={makeRef({ lockHash: LOCK_HASH })} compact />);
    const link = screen.getByRole('link');
    expect(link).toHaveAttribute('href', `/mainnet/address/${LOCK_HASH}`);
    expect(link).toHaveTextContent(truncateHash(LOCK_HASH, 8, 6));
  });

  it('refuses a party that carries no identity at all instead of rendering an empty label', () => {
    expect(() =>
      render(<ParticipantRefView participant={makeRef({ roles: ['owner_to'] })} />)
    ).toThrow(/no address, lock hash or lock-hash prefix/);
  });

  it('shows the protocol roles it was given', () => {
    render(
      <ParticipantRefView
        participant={makeRef({ lockHashPrefix: '0xdead', roles: ['owner_to', 'manager_to'] })}
      />
    );
    expect(screen.getByText(/owner_to/)).toBeInTheDocument();
    expect(screen.getByText(/manager_to/)).toBeInTheDocument();
  });
});
