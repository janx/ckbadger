// Round 3 lane F: banners for `store_catching_up` (contracts S) and `derive_pending` (L0).
import { afterEach, describe, expect, it } from 'vitest';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider, useQuery } from '@tanstack/react-query';
import { ApiRequestError } from '@/lib/api';
import { DerivePendingBanner, StoreCatchingUpBanner } from '@/components/ui/store-state-banner';

function ThrowingQuery({ error }: { error: unknown }) {
  const { status } = useQuery({
    queryKey: ['store-state-banner-test'],
    queryFn: async () => {
      throw error;
    },
  });
  return <div data-testid="query-status">{status}</div>;
}

function renderBanners(error: unknown) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: Infinity } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <StoreCatchingUpBanner />
      <DerivePendingBanner />
      <ThrowingQuery error={error} />
    </QueryClientProvider>
  );
}

afterEach(cleanup);

describe('StoreCatchingUpBanner', () => {
  it('says the store is catching up and quotes the API', async () => {
    renderBanners(
      new ApiRequestError(
        503,
        'store_catching_up',
        'domain store catch-up with the indexer failed 3 consecutive times; last error: io'
      )
    );
    const banner = await screen.findByRole('status', { name: 'Catching up' });
    expect(banner).toHaveTextContent('Catching up with the indexer. Retrying automatically.');
    expect(banner).toHaveTextContent('failed 3 consecutive times; last error: io');
    expect(screen.queryByRole('status', { name: 'Being derived' })).toBeNull();
  });

  it('renders nothing for other errors', async () => {
    renderBanners(new ApiRequestError(500, 'internal_error', 'boom'));
    await waitFor(() => expect(screen.getByTestId('query-status')).toHaveTextContent('error'));
    expect(screen.queryByRole('status')).toBeNull();
  });
});

describe('DerivePendingBanner', () => {
  it('names the aggregate and the exact progress the indexer reported', async () => {
    renderBanners(
      new ApiRequestError(503, 'derive_pending', 'dao_live_agg is being derived', {
        aggregate: 'dao_live_agg',
        state: 'running',
        rowsDone: 12000,
        derivedFromTip: 20912345,
      })
    );
    const banner = await screen.findByRole('status', { name: 'Being derived' });
    expect(banner).toHaveTextContent(
      'The indexer is deriving the DAO depositor totals: 12,000 source rows folded, from the chain at block #20,912,345.'
    );
    expect(screen.queryByRole('status', { name: 'Catching up' })).toBeNull();
  });

  it('says a derivation has not started or is being redone, without inventing progress', async () => {
    renderBanners(
      new ApiRequestError(503, 'derive_pending', 'x', {
        aggregate: 'bit_cell_by_account',
        state: 'outdated',
        rowsDone: null,
        derivedFromTip: null,
      })
    );
    const banner = await screen.findByRole('status', { name: 'Being derived' });
    expect(banner).toHaveTextContent(
      'The indexer is deriving the .bit Cell index again: the stored one predates this version.'
    );
    expect(banner).not.toHaveTextContent('rows');
  });
});
