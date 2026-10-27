import { fireEvent, render, screen } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { describe, expect, it } from 'vitest';
import { http, HttpResponse } from 'msw';
import { server } from '@/__tests__/msw/server';
import { layoutMapPoints, NetworkMapView } from '@/app/network/network-map';
import type { NetworkMap } from '@/lib/api';
import producerMap from '@/__tests__/fixtures/network-api-producer/network-map.json';

const first: NetworkMap['points'][number] = {
  peerId: 'a',
  country: 'US',
  lat: 40,
  lon: -70,
  reachable: true,
  clientVersion: 'ckb/0.114',
  asn: 'AS1 Example',
  firstSeen: 100,
  lastSeen: 200,
  advertiserCount: 2,
  localSessionObserved: true,
};
const second = { ...first, peerId: 'b', reachable: false, clientVersion: 'ckb/0.113' };

describe('NetworkMapView', () => {
  it('renders the API producer map payload with retained scope and unknown location count', async () => {
    server.use(http.get('/api/:network/v1/network/map', () => HttpResponse.json(producerMap)));
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <NetworkMapView />
      </QueryClientProvider>
    );
    expect(await screen.findByText('0.119.0')).toBeInTheDocument();
    expect(screen.getByText('AS65000 Ex')).toBeInTheDocument();
    expect(screen.getByText(/1 of 2 retained verified peers/)).toHaveTextContent(
      '1 locations are unavailable'
    );
  });

  it('displaces coincident dots deterministically without changing source coordinates', () => {
    const layout = layoutMapPoints([second, first]);
    expect(layout.map(({ point }) => point.peerId)).toEqual(['a', 'b']);
    expect([layout[0].x, layout[0].y]).not.toEqual([layout[1].x, layout[1].y]);
    expect(Math.hypot(layout[0].x - layout[1].x, layout[0].y - layout[1].y)).toBeGreaterThanOrEqual(
      8
    );
    expect(layout[0].point.lat).toBe(40);
    expect(layout[1].point.lon).toBe(-70);
  });

  it('shows focused peer evidence and country count', async () => {
    server.use(
      http.get('/api/:network/v1/network/map', () =>
        HttpResponse.json({
          roundId: 5,
          verifiedRetained: 2,
          located: 2,
          locationUnavailable: 0,
          points: [first, second],
        })
      )
    );
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <NetworkMapView />
      </QueryClientProvider>
    );
    const peer = await screen.findByLabelText(
      'US peer b, verified before but unavailable in last round'
    );
    fireEvent.focus(peer);
    const detail = screen.getByRole('complementary', { name: 'Selected peer evidence' });
    expect(detail).toHaveTextContent('ckb/0.113');
    expect(detail).toHaveTextContent('verified earlier; unavailable last round');
    expect(detail).toHaveTextContent('2 retained in round #5');
    expect(screen.getByRole('list', { name: 'Located peers by country' })).toHaveTextContent(
      'US 2'
    );
  });
});
