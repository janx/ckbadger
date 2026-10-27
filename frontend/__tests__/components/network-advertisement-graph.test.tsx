import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { http, HttpResponse } from 'msw';
import { server } from '@/__tests__/msw/server';
import { AdvertisementGraph } from '@/app/network/advertisement-graph';

describe('AdvertisementGraph', () => {
  it('reconciles a newer completed round instead of leaving a stale-round message forever', async () => {
    server.use(
      http.get('/api/:network/v1/network/advertisements', () =>
        HttpResponse.json({
          roundId: 6,
          finishedAt: 300,
          coverage: 'retainedLatestObservationOnly',
          distinctPeerPairs: 0,
          edges: [],
        })
      )
    );
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const invalidate = vi.spyOn(client, 'invalidateQueries');
    render(
      <QueryClientProvider client={client}>
        <AdvertisementGraph roundId={5} />
      </QueryClientProvider>
    );
    expect(await screen.findByRole('status')).toHaveTextContent('Crawler round changed');
    await waitFor(() =>
      expect(invalidate).toHaveBeenCalledWith({ queryKey: ['network', 'summary'] })
    );
  });

  it('makes every retained alias observation reachable and distinguishes pairs from aliases', async () => {
    const edges = Array.from({ length: 25 }, (_, index) => ({
      advertiserPeerId: 'peer-a',
      targetPeerId: 'peer-b',
      alias: `alias-${index}`,
      observedAt: 200,
    }));
    server.use(
      http.get('/api/:network/v1/network/advertisements', () =>
        HttpResponse.json({
          roundId: 5,
          finishedAt: 200,
          coverage: 'retainedLatestObservationOnly',
          distinctPeerPairs: 1,
          edges,
        })
      )
    );
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <AdvertisementGraph roundId={5} />
      </QueryClientProvider>
    );
    expect(await screen.findByText(/1 unique advertiser/i)).toHaveTextContent(
      '25 retained endpoint alias observations'
    );
    expect(screen.getAllByRole('listitem')).toHaveLength(24);
    fireEvent.click(screen.getByRole('button', { name: 'Show all 25 observations' }));
    expect(screen.getAllByRole('listitem')).toHaveLength(25);
    expect(screen.getByText('alias-24')).toBeInTheDocument();
  });
});
