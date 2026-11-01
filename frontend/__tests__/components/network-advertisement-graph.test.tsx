import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { http, HttpResponse } from 'msw';
import { server } from '@/__tests__/msw/server';
import { AdvertisementGraph } from '@/app/network/advertisement-graph';
import producerAdvertisements from '@/__tests__/fixtures/network-api-producer/network-advertisements.json';

describe('AdvertisementGraph', () => {
  it('renders the latest-round producer payload as a directed, scoped observation', async () => {
    server.use(
      http.get('/api/:network/v1/network/advertisements', () =>
        HttpResponse.json(producerAdvertisements)
      )
    );
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <AdvertisementGraph roundId={5} />
      </QueryClientProvider>
    );
    expect(await screen.findByText('addrpeerC')).toBeInTheDocument();
    expect(screen.getByText(/1 unique advertiser/i)).toHaveTextContent(
      '1 retained endpoint alias observations'
    );
    expect(screen.getByRole('listitem')).toHaveTextContent('7065657241');
  });

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

  it('truncates peer ids in the middle and wraps the multiaddr instead of cutting it (T3)', async () => {
    const advertiser = `1220094f5698529bc8f3${'a'.repeat(40)}0badc0de`;
    const target = `122007d7be${'b'.repeat(50)}b23985be`;
    const alias = '/ip4/54.156.243.239/tcp/8115/p2p/QmNsGNQjYA6iP472bNnNE2GR31kCYBifhY1XcaUxRjZ1py';
    server.use(
      http.get('/api/:network/v1/network/advertisements', () =>
        HttpResponse.json({
          roundId: 5,
          finishedAt: 200,
          coverage: 'retainedLatestObservationOnly',
          distinctPeerPairs: 1,
          edges: [{ advertiserPeerId: advertiser, targetPeerId: target, alias, observedAt: 200 }],
        })
      )
    );
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <AdvertisementGraph roundId={5} />
      </QueryClientProvider>
    );
    const head = await screen.findByTitle(advertiser);
    expect(head).toHaveTextContent(/^1220094f56\.\.\.0badc0de$/);
    expect(screen.getByTitle(target)).toHaveTextContent(/^122007d7be\.\.\.b23985be$/);
    const addr = screen.getByText(alias);
    expect(addr).toHaveClass('[overflow-wrap:anywhere]');
    expect(addr).not.toHaveClass('truncate');
  });
});
