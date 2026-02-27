import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';

import { NftCollectionStatCards } from '@/components/nft/nft-collection-stat-cards';
import { render } from '../utils/test-utils';

describe('NftCollectionStatCards', () => {
  it('renders total count with default label', () => {
    render(
      <NftCollectionStatCards
        totalCount={500}
        liveCapacity="100000000000"
        liveOccupiedCapacity="61000000000"
      />
    );

    expect(screen.getByText('Total NFTs')).toBeInTheDocument();
    expect(screen.getByText('500')).toBeInTheDocument();
  });

  it('renders custom total label', () => {
    render(
      <NftCollectionStatCards
        totalCount={42}
        totalLabel="Total Spores"
        liveCapacity="100000000000"
        liveOccupiedCapacity="61000000000"
      />
    );

    expect(screen.getByText('Total Spores')).toBeInTheDocument();
  });

  it('renders capacity and occupied capacity', () => {
    render(
      <NftCollectionStatCards
        totalCount={10}
        liveCapacity="100000000000"
        liveOccupiedCapacity="61000000000"
      />
    );

    expect(screen.getByText('Live Capacity')).toBeInTheDocument();
    expect(screen.getByText('Occupied Capacity')).toBeInTheDocument();
    expect(screen.getByText(/Occupied Ratio: 61\.00%/)).toBeInTheDocument();
  });

  it('renders storage tier when provided', () => {
    render(
      <NftCollectionStatCards
        totalCount={10}
        liveCapacity={null}
        liveOccupiedCapacity={null}
        storageTier="fully_onchain"
        storageOnchainRatio="0.95"
      />
    );

    expect(screen.getByText('Storage Integrity')).toBeInTheDocument();
    expect(screen.getByText('Fully On-chain')).toBeInTheDocument();
    expect(screen.getByText(/On-chain ratio: 95\.00%/)).toBeInTheDocument();
  });

  it('renders created at block when provided', () => {
    render(
      <NftCollectionStatCards
        totalCount={10}
        liveCapacity={null}
        liveOccupiedCapacity={null}
        createdAtBlock={1000000}
      />
    );

    expect(screen.getByText('Created At')).toBeInTheDocument();
    const blockLink = screen.getByRole('link', { name: /#1,000,000/ });
    expect(blockLink).toHaveAttribute('href', '/blocks/1000000');
  });

  it('shows dashes when capacity is null', () => {
    render(
      <NftCollectionStatCards totalCount={10} liveCapacity={null} liveOccupiedCapacity={null} />
    );

    const dashes = screen.getAllByText('--');
    expect(dashes.length).toBeGreaterThanOrEqual(2);
  });

  it('does not render storage card when storageTier is not provided', () => {
    render(
      <NftCollectionStatCards totalCount={10} liveCapacity={null} liveOccupiedCapacity={null} />
    );

    expect(screen.queryByText('Storage Integrity')).not.toBeInTheDocument();
  });

  it('does not render created at card when createdAtBlock is not provided', () => {
    render(
      <NftCollectionStatCards totalCount={10} liveCapacity={null} liveOccupiedCapacity={null} />
    );

    expect(screen.queryByText('Created At')).not.toBeInTheDocument();
  });
});
