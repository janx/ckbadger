import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';

import { ObjectCollectionStatCards } from '@/components/object/object-collection-stat-cards';
import { render } from '../utils/test-utils';

describe('ObjectCollectionStatCards', () => {
  it('renders default count card', () => {
    render(<ObjectCollectionStatCards totalCount={500} />);

    expect(screen.getByText('Total Objects')).toBeInTheDocument();
    expect(screen.getByText('500')).toBeInTheDocument();
  });

  it('renders storage tier when provided', () => {
    render(
      <ObjectCollectionStatCards
        totalCount={10}
        compositionTier="btc_ckb"
        storageOnchainRatio="0.95"
      />
    );

    expect(screen.getByText('Storage Integrity')).toBeInTheDocument();
    expect(screen.getByText('BTC+CKB')).toBeInTheDocument();
    expect(screen.getByText(/On-chain ratio: 95\.00%/)).toBeInTheDocument();
  });

  it('renders custom total label and created-at block link when provided', () => {
    render(
      <ObjectCollectionStatCards
        totalCount={42}
        totalLabel="Total Spores"
        createdAtBlock={1000000}
      />
    );

    expect(screen.getByText('Total Spores')).toBeInTheDocument();
    expect(screen.getByText('Created At')).toBeInTheDocument();
    const blockLink = screen.getByRole('link', { name: /#1,000,000/ });
    expect(blockLink).toHaveAttribute('href', '/mainnet/blocks/1000000');
  });

  it('hides optional cards when optional props are missing', () => {
    render(<ObjectCollectionStatCards totalCount={10} />);

    expect(screen.queryByText('Storage Integrity')).not.toBeInTheDocument();
    expect(screen.queryByText('Created At')).not.toBeInTheDocument();
  });

  it('shows live count when different from total', () => {
    render(<ObjectCollectionStatCards totalCount={100} liveCount={80} />);

    expect(screen.getByText('Live Items')).toBeInTheDocument();
    expect(screen.getByText('80')).toBeInTheDocument();
  });

  it('hides live count when equal to total', () => {
    render(<ObjectCollectionStatCards totalCount={100} liveCount={100} />);

    expect(screen.queryByText('Live Items')).not.toBeInTheDocument();
  });
});
