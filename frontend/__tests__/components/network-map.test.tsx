import { fireEvent, render, screen } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { describe, expect, it } from 'vitest';
import { http, HttpResponse } from 'msw';
import { server } from '@/__tests__/msw/server';
import { layoutMapPoints, NetworkMapNotes, NetworkMapView } from '@/app/network/network-map';
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
  // Round 3 (R-13): present exactly when localSessionObserved is.
  localSession: {
    connectedDurationMs: 60_000,
    observedAt: 200,
    initiator: 'peerInitiated',
  },
};
const second = { ...first, peerId: 'b', reachable: false, clientVersion: 'ckb/0.113' };

describe('NetworkMapView', () => {
  it('renders the API producer map payload with retained scope and unknown location count', async () => {
    server.use(http.get('/api/:network/v1/network/map', () => HttpResponse.json(producerMap)));
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <NetworkMapView neverVerified={0} />
        <NetworkMapNotes />
      </QueryClientProvider>
    );
    // F2 charts (CH-51): the panel starts as the legend; a peer's readout follows focus.
    fireEvent.focus(await screen.findByLabelText('US peer 7065657241, reachable now'));
    const detail = screen.getByRole('complementary', { name: 'Selected peer evidence' });
    expect(detail).toHaveTextContent('0.119.0');
    expect(detail).toHaveTextContent('US · AS65000 Ex');
    // Hierarchy v3 (N2): the coverage note is tier 2's, in the map notes.
    expect(screen.getByText(/1 of 2 retained verified peers/)).toHaveTextContent(
      '1 locations are unavailable'
    );
  });

  // Midnight, continued (review 2026-11-03, item 5, move 3): the map is an instrument, in the one
  // frame; the peer evidence rail beside it stays flat.
  it('draws the map inside the instrument frame, measured within it (move 3)', async () => {
    server.use(http.get('/api/:network/v1/network/map', () => HttpResponse.json(producerMap)));
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <NetworkMapView neverVerified={0} />
      </QueryClientProvider>
    );
    const map = await screen.findByRole('img', {
      name: 'Approximate GeoIP locations of retained verified CKB peers',
    });
    const frame = map.closest('[data-instrument-frame]') as HTMLElement;
    expect(frame).not.toBeNull();
    expect(frame).toHaveAttribute('aria-label', 'Map of verified peers');
    expect(frame).toHaveClass('neon-edge-top', 'rounded', 'border', 'border-base-border');
    expect(frame).not.toContainElement(
      screen.getByRole('complementary', { name: 'Selected peer evidence' })
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
        <NetworkMapView neverVerified={0} />
      </QueryClientProvider>
    );
    const peer = await screen.findByLabelText(
      'US peer b, verified before, not reachable now, a session with this node in the latest round'
    );
    fireEvent.focus(peer);
    const detail = screen.getByRole('complementary', { name: 'Selected peer evidence' });
    expect(detail).toHaveTextContent('ckb/0.113');
    expect(detail).toHaveTextContent('verified before, not reachable now');
    expect(detail).toHaveTextContent('of 2 peers in round #5');
    // The country count sits at its cluster on the map (CH-51), not in a list under it.
    expect(
      [...document.querySelectorAll('[data-country-label]')].map((node) => node.textContent)
    ).toEqual(['US 2']);
  });
});
