import { afterEach, describe, expect, it } from 'vitest';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider, useQuery } from '@tanstack/react-query';
import { ApiRequestError } from '@/lib/api';
import { NetworkInitializingBanner } from '@/components/ui/network-initializing-banner';

function ThrowingQuery({ error }: { error: unknown }) {
  const { status } = useQuery({
    queryKey: ['network-initializing-banner-test'],
    queryFn: async () => {
      throw error;
    },
  });
  return <div data-testid="query-status">{status}</div>;
}

function renderBanner(error: unknown) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: Infinity } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <NetworkInitializingBanner />
      <ThrowingQuery error={error} />
    </QueryClientProvider>
  );
}

afterEach(cleanup);

describe('NetworkInitializingBanner', () => {
  it('shows a waiting-to-sync notice with the API message when a query reports initializing', async () => {
    renderBanner(
      new ApiRequestError(503, 'initializing', 'This network has not started syncing yet')
    );
    expect(await screen.findByText('Waiting to sync')).toBeInTheDocument();
    expect(screen.getByText('This network has not started syncing yet')).toBeInTheDocument();
  });

  it('renders nothing for unrelated errors', async () => {
    renderBanner(new ApiRequestError(500, 'boom', 'unexpected'));
    await waitFor(() => expect(screen.getByTestId('query-status')).toHaveTextContent('error'));
    expect(screen.queryByText('Waiting to sync')).not.toBeInTheDocument();
  });
});
