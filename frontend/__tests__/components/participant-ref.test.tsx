import { render, screen } from '@testing-library/react';
import { ParticipantRefView } from '@/components/participant-ref';
import type { ParticipantRef } from '@/lib/api';

function makeRef(overrides: Partial<ParticipantRef> = {}): ParticipantRef {
  return {
    address: overrides.address ?? null,
    lockHash: overrides.lockHash ?? null,
    lockHashPrefix: overrides.lockHashPrefix ?? null,
    roles: overrides.roles ?? [],
    sent: overrides.sent ?? true,
  };
}

describe('ParticipantRefView', () => {
  const address =
    'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqt4z78ng4yutl5u6xsv27lt52eh9jvvtd9wj5clj';

  // re-pinned on purpose (lane cd-fixes-controls, review-p-tests SHOULD-4): the full form is an
  // Address, which states its look
  it('renders a linked address when the party resolved', () => {
    render(
      <ParticipantRefView
        size="row"
        look="inline"
        participant={makeRef({ address, lockHash: '0xabcd' })}
      />
    );
    const link = screen.getByRole('link');
    expect(link).toHaveAttribute('href', `/mainnet/address/${address}`);
  });

  it('draws a compact address in jade, or in the plain ink when quiet', () => {
    const { unmount } = render(
      <ParticipantRefView size="row" participant={makeRef({ address })} compact />
    );
    // R4 (re-pinned on purpose): the jade's dim step, a named ink at 4.5:1, not jade at 80 %
    expect(screen.getByRole('link')).toHaveClass('text-jade-dim');
    unmount();
    render(<ParticipantRefView size="row" participant={makeRef({ address })} compact quiet />);
    const link = screen.getByRole('link');
    expect(link).toHaveClass('text-text');
    expect(link).not.toHaveClass('text-jade-dim');
    expect(link).toHaveAttribute('href', `/mainnet/address/${address}`);
  });

  it('renders the prefix and an unresolved marker when it did not, with no link', () => {
    const prefix = '0x1e3a88ca5cc39f1bd38c091b53e33b7c29ebd019';
    render(<ParticipantRefView size="row" participant={makeRef({ lockHashPrefix: prefix })} />);
    expect(screen.queryByRole('link')).toBeNull();
    expect(screen.getByText(/unresolved/i)).toBeInTheDocument();
    // The full prefix stays reachable even though the label is shortened.
    expect(screen.getByTitle(prefix)).toBeInTheDocument();
  });

  // U-09 (re-pinned): every id the party writes takes the size its row gives it
  it('shortens an unresolved prefix the way every other hash is shortened', () => {
    const prefix = '0x1e3a88ca5cc39f1bd38c091b53e33b7c29ebd019';
    render(<ParticipantRefView size="row" participant={makeRef({ lockHashPrefix: prefix })} />);
    expect(screen.getByTitle(prefix)).toHaveTextContent(/^0x1e3a88ca…ebd019$/);
  });

  // A Lock party whose lock script the store does not know encodes to no
  // address; the API sends `address: null` with the lock hash it does know.
  const LOCK_HASH = '0x57d926a44d83fc13b21ce037b1e31f4223e3c867cfa3f60e1324d5bfd5cd742d';

  it('shows a lock party without an address as its lock hash, linked by hash', () => {
    render(<ParticipantRefView size="row" participant={makeRef({ lockHash: LOCK_HASH })} />);
    const link = screen.getByRole('link');
    expect(link).toHaveAttribute('href', `/mainnet/address/${LOCK_HASH}`);
    expect(link).toHaveAttribute('title', LOCK_HASH);
    expect(link).toHaveTextContent(/^0x57d926a4…cd742d$/);
    // It is resolved to a lock, just not to an address.
    expect(screen.queryByText(/unresolved/i)).toBeNull();
  });

  it('shows a lock party without an address as its lock hash in the compact form too', () => {
    const { unmount } = render(
      <ParticipantRefView size="row" participant={makeRef({ lockHash: LOCK_HASH })} compact />
    );
    const link = screen.getByRole('link');
    expect(link).toHaveAttribute('href', `/mainnet/address/${LOCK_HASH}`);
    expect(link).toHaveTextContent(/^0x57d926a4…cd742d$/);
    unmount();
    render(
      <ParticipantRefView size="tight" participant={makeRef({ lockHash: LOCK_HASH })} compact />
    );
    expect(screen.getByRole('link')).toHaveTextContent(/^0x57d9…742d$/);
  });

  it('refuses a party that carries no identity at all instead of rendering an empty label', () => {
    expect(() =>
      render(<ParticipantRefView size="row" participant={makeRef({ roles: ['owner_to'] })} />)
    ).toThrow(/no address, lock hash or lock-hash prefix/);
  });

  // Consistency C3: the party a page is about (the address page's own line) is text, not a link
  // to the page it is on; the whole address or lock hash stays in its hover, the roles beside it.
  it('writes the compact party as text with the full value in its hover when not linked', () => {
    const { unmount } = render(
      <ParticipantRefView
        size="row"
        participant={makeRef({ address, lockHash: LOCK_HASH, roles: ['owner'] })}
        compact
        linked={false}
      />
    );
    expect(screen.queryByRole('link')).toBeNull();
    const text = screen.getByTitle(address);
    expect(text).toHaveTextContent(/^ckb1…d9wj5clj$/); // R9 (re-pinned): 4…8 at the row size
    expect(text).toHaveClass('text-text-dim', 'font-mono');
    expect(screen.getByText('owner')).toBeInTheDocument();
    unmount();
    render(
      <ParticipantRefView
        size="row"
        participant={makeRef({ lockHash: LOCK_HASH })}
        compact
        linked={false}
      />
    );
    expect(screen.queryByRole('link')).toBeNull();
    expect(screen.getByTitle(LOCK_HASH)).toHaveTextContent(/^0x57d926a4…cd742d$/);
  });

  it('refuses the unlinked form outside the compact activity-row line', () => {
    expect(() =>
      render(<ParticipantRefView size="row" participant={makeRef({ address })} linked={false} />)
    ).toThrow(/compact/);
  });

  it('shows the protocol roles it was given', () => {
    render(
      <ParticipantRefView
        size="row"
        participant={makeRef({ lockHashPrefix: '0xdead', roles: ['owner_to', 'manager_to'] })}
      />
    );
    expect(screen.getByText(/owner_to/)).toBeInTheDocument();
    expect(screen.getByText(/manager_to/)).toBeInTheDocument();
  });

  // Lane P1 follow-up (the link-look gate's participant-ref entry): a caller states the party's
  // look (components/ui/link.tsx) as it does an Address's; the link then draws that look and no ink
  // by hand. A caller that states none draws what it drew (the feeds, the lists lane's).
  describe('the look its caller states', () => {
    const LOCK = `0x${'57'.repeat(32)}`;
    const INLINE_LINE = 'decoration-[color:var(--c-ink-muted)]';

    it('draws a compact address in the inline look, with no hand ink', () => {
      render(
        <ParticipantRefView
          size="row"
          participant={makeRef({ address })}
          compact
          quiet
          look="inline"
        />
      );
      const link = screen.getByRole('link');
      expect(link).toHaveClass('underline', INLINE_LINE);
      for (const hand of ['text-text', 'text-jade-dim', 'hover:text-jade'])
        expect(link).not.toHaveClass(hand);
    });

    it('draws a compact lock hash as a row key, and a standalone address too', () => {
      const { unmount } = render(
        <ParticipantRefView
          size="row"
          participant={makeRef({ lockHash: LOCK })}
          compact
          look="key"
        />
      );
      expect(screen.getByRole('link')).toHaveClass('text-text-bright');
      expect(screen.getByRole('link')).not.toHaveClass('hover:text-jade');
      unmount();
      render(<ParticipantRefView size="row" participant={makeRef({ address })} look="key" />);
      expect(screen.getByRole('link')).toHaveClass('text-text-bright');
      expect(screen.getByRole('link')).not.toHaveClass('underline');
    });

    it('draws what it drew where its caller states no look', () => {
      render(<ParticipantRefView size="row" participant={makeRef({ lockHash: LOCK })} compact />);
      expect(screen.getByRole('link')).toHaveClass('text-text', 'hover:text-jade');
    });
  });
});
