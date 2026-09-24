import { render, screen } from '@testing-library/react';
import { ParticipantRefView } from '@/components/participant-ref';
import type { ParticipantRef } from '@/lib/api';

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
