import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';

import { NftActivityCard } from '@/components/nft/nft-activity-card';
import { render } from '../utils/test-utils';

describe('NftActivityCard', () => {
  it('renders block link, tx link, and plain text actions', () => {
    render(<NftActivityCard txHash="0xabc123" blockNumber={456} actions={['transfer', 'mint']} />);

    const blockLink = screen.getByRole('link', { name: '#456' });
    expect(blockLink).toHaveAttribute('href', '/blocks/456');

    expect(screen.getByText('transfer, mint')).toBeInTheDocument();
  });

  it('renders tx index when provided', () => {
    render(<NftActivityCard txHash="0xabc123" blockNumber={100} txIndex={3} actions={['mint']} />);

    expect(screen.getByText(/Tx Index 3/)).toBeInTheDocument();
  });

  it('renders timestamp when provided', () => {
    render(
      <NftActivityCard
        txHash="0xabc123"
        blockNumber={100}
        timestamp="2023-01-01T00:00:00Z"
        actions={['mint']}
      />
    );

    expect(screen.getByText(/Timestamp:/)).toBeInTheDocument();
  });

  it('applies normalizeAction to action labels', () => {
    render(
      <NftActivityCard
        txHash="0xabc123"
        blockNumber={100}
        actions={['burn', 'transfer']}
        normalizeAction={(a) => (a === 'burn' ? 'recycled' : a)}
      />
    );

    expect(screen.getByText('recycled, transfer')).toBeInTheDocument();
  });

  it('renders Badge components when badgeActions is true', () => {
    render(
      <NftActivityCard
        txHash="0xabc123"
        blockNumber={100}
        actions={['mint', 'burn']}
        badgeActions
      />
    );

    expect(screen.getByText('mint')).toBeInTheDocument();
    expect(screen.getByText('burn')).toBeInTheDocument();
  });

  it('does not render timestamp when not provided', () => {
    render(<NftActivityCard txHash="0xabc123" blockNumber={100} actions={['mint']} />);

    expect(screen.queryByText(/Timestamp:/)).not.toBeInTheDocument();
  });
});
