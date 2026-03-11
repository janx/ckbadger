import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';

import { ObjectCollectionStatCards } from '@/components/object/object-collection-stat-cards';
import { render } from '../utils/test-utils';

describe('ObjectCollectionStatCards', () => {
  it('renders total count with default label', () => {
    render(
      <ObjectCollectionStatCards
        totalCount={500}
        liveCapacity="100000000000"
        liveUsedCapacity="61000000000"
      />
    );

    expect(screen.getByText('Total Objects')).toBeInTheDocument();
    expect(screen.getByText('500')).toBeInTheDocument();
  });

  it('renders custom total label', () => {
    render(
      <ObjectCollectionStatCards
        totalCount={42}
        totalLabel="Total Spores"
        liveCapacity="100000000000"
        liveUsedCapacity="61000000000"
      />
    );

    expect(screen.getByText('Total Spores')).toBeInTheDocument();
  });

  it('renders capacity and used capacity', () => {
    render(
      <ObjectCollectionStatCards
        totalCount={10}
        liveCapacity="100000000000"
        liveUsedCapacity="61000000000"
      />
    );

    expect(screen.getByText('Live Capacity')).toBeInTheDocument();
    expect(screen.getByText('Used Capacity')).toBeInTheDocument();
    expect(screen.getByText(/Used Ratio: 61\.00%/)).toBeInTheDocument();
  });

  it('renders storage tier when provided', () => {
    render(
      <ObjectCollectionStatCards
        totalCount={10}
        liveCapacity={null}
        liveUsedCapacity={null}
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
      <ObjectCollectionStatCards
        totalCount={10}
        liveCapacity={null}
        liveUsedCapacity={null}
        createdAtBlock={1000000}
      />
    );

    expect(screen.getByText('Created At')).toBeInTheDocument();
    const blockLink = screen.getByRole('link', { name: /#1,000,000/ });
    expect(blockLink).toHaveAttribute('href', '/blocks/1000000');
  });

  it('shows dashes when capacity is null', () => {
    render(
      <ObjectCollectionStatCards totalCount={10} liveCapacity={null} liveUsedCapacity={null} />
    );

    const dashes = screen.getAllByText('--');
    expect(dashes.length).toBeGreaterThanOrEqual(2);
  });

  it('does not render storage card when storageTier is not provided', () => {
    render(
      <ObjectCollectionStatCards totalCount={10} liveCapacity={null} liveUsedCapacity={null} />
    );

    expect(screen.queryByText('Storage Integrity')).not.toBeInTheDocument();
  });

  it('does not render created at card when createdAtBlock is not provided', () => {
    render(
      <ObjectCollectionStatCards totalCount={10} liveCapacity={null} liveUsedCapacity={null} />
    );

    expect(screen.queryByText('Created At')).not.toBeInTheDocument();
  });
});
