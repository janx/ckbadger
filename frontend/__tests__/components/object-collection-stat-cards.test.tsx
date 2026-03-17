import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';

import { ObjectCollectionStatCards } from '@/components/object/object-collection-stat-cards';
import { render } from '../utils/test-utils';

describe('ObjectCollectionStatCards', () => {
  it('renders default count and capacity cards', () => {
    render(
      <ObjectCollectionStatCards
        totalCount={500}
        liveCapacity="100000000000"
        liveCommonKnowledgeSize="61000000000"
      />
    );

    expect(screen.getByText('Total Objects')).toBeInTheDocument();
    expect(screen.getByText('500')).toBeInTheDocument();
    expect(screen.getByText('Live Capacity')).toBeInTheDocument();
    expect(screen.getByText('Common Knowledge Size')).toBeInTheDocument();
    expect(screen.getByText(/Common Knowledge Share: 61\.00%/)).toBeInTheDocument();
  });

  it('renders storage tier when provided', () => {
    render(
      <ObjectCollectionStatCards
        totalCount={10}
        liveCapacity={null}
        liveCommonKnowledgeSize={null}
        storageTier="fully_onchain"
        storageOnchainRatio="0.95"
      />
    );

    expect(screen.getByText('Storage Integrity')).toBeInTheDocument();
    expect(screen.getByText('Fully On-chain')).toBeInTheDocument();
    expect(screen.getByText(/On-chain ratio: 95\.00%/)).toBeInTheDocument();
  });

  it('renders custom total label and created-at block link when provided', () => {
    render(
      <ObjectCollectionStatCards
        totalCount={42}
        totalLabel="Total Spores"
        liveCapacity={null}
        liveCommonKnowledgeSize={null}
        createdAtBlock={1000000}
      />
    );

    expect(screen.getByText('Total Spores')).toBeInTheDocument();
    expect(screen.getByText('Created At')).toBeInTheDocument();
    const blockLink = screen.getByRole('link', { name: /#1,000,000/ });
    expect(blockLink).toHaveAttribute('href', '/blocks/1000000');
  });

  it('shows fallback values and hides optional cards when optional props are missing', () => {
    render(
      <ObjectCollectionStatCards
        totalCount={10}
        liveCapacity={null}
        liveCommonKnowledgeSize={null}
      />
    );

    const dashes = screen.getAllByText('--');
    expect(dashes.length).toBeGreaterThanOrEqual(2);
    expect(screen.queryByText('Storage Integrity')).not.toBeInTheDocument();
    expect(screen.queryByText('Created At')).not.toBeInTheDocument();
  });
});
