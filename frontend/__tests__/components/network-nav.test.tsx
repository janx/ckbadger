import { describe, it, expect, vi } from 'vitest';
import { http, HttpResponse } from 'msw';
import { render, screen } from '../utils/test-utils';
import { server } from '../msw/server';
import { Header } from '@/components/layout/header';
import { NetworkClientPage } from '@/app/network/client-page';

const API_BASE = '/api/:network/v1';

// Mirror header.test.tsx: stub the header's heavy sub-widgets and the pathname hook so the real
// nav (and the real Header rendered inside the Peers page) mounts without Router/stats plumbing.
const usePathnameMock = vi.fn(() => '/network');
vi.mock('@/src/navigation', () => ({
  usePathname: () => usePathnameMock(),
}));
vi.mock('@/components/command-palette', () => ({
  CommandPalette: () => <div data-testid="command-palette" />,
}));
vi.mock('@/components/search-bar', () => ({
  SearchBar: () => <div data-testid="search-bar" />,
}));
vi.mock('@/components/layout/logo', () => ({
  Logo: () => <div data-testid="logo" />,
}));
vi.mock('@/components/stats-bar', () => ({
  GlobalStatsBar: () => <div data-testid="global-stats-bar" />,
}));

// The attribution string uses a real © glyph — match the whole rendered <p> text.
const ATTRIBUTION = /Geo\/ASN data © MaxMind \(GeoLite2\)/i;

const DASHBOARD_SUMMARY = {
  enabled: true,
  hasData: true,
  lastRound: {
    roundId: 5,
    started: 0,
    finished: 0,
    dialed: 2,
    reachable: 1,
    unreachable: 1,
    foreignDropped: 0,
    newNodes: 0,
    totalKnown: 2,
    frontierDrained: true,
  },
};

describe('Peers nav entry', () => {
  // Peers was removed from the navbar; it is now reachable via the `g p` keyboard
  // shortcut and the command palette (see command-palette.test.tsx).
  it('does not render a "Peers" link in the navbar', () => {
    render(<Header />);

    expect(screen.queryByRole('link', { name: 'Peers' })).not.toBeInTheDocument();
  });
});

describe('MaxMind attribution', () => {
  it('renders the MaxMind attribution when geo data is present', async () => {
    server.use(
      http.get(`${API_BASE}/network/summary`, () => HttpResponse.json(DASHBOARD_SUMMARY)),
      http.get(`${API_BASE}/network/distributions`, () =>
        HttpResponse.json({
          totalKnown: 2,
          reachable: 1,
          unreachable: 1,
          versions: [{ label: '0.114.0', count: 2 }],
          countries: [{ label: 'United States', count: 2 }],
          asns: [{ label: 'AS24940 Hetzner', count: 2 }],
          protocols: [{ label: '/ckb/2', count: 2 }],
        })
      )
    );

    render(<NetworkClientPage />);

    expect(await screen.findByText(ATTRIBUTION)).toBeInTheDocument();
  });

  it('does not render the attribution in the onboarding / no-data state', async () => {
    // Default MSW summary => { enabled:false, hasData:false } => onboarding, no dashboard.
    render(<NetworkClientPage />);

    expect(await screen.findByText('The CKB peer crawler')).toBeInTheDocument();
    expect(screen.queryByText(ATTRIBUTION)).not.toBeInTheDocument();
  });

  it('does not render the attribution when geo is empty (only "Unknown")', async () => {
    server.use(
      http.get(`${API_BASE}/network/summary`, () => HttpResponse.json(DASHBOARD_SUMMARY)),
      http.get(`${API_BASE}/network/distributions`, () =>
        HttpResponse.json({
          totalKnown: 2,
          reachable: 1,
          unreachable: 1,
          versions: [{ label: '0.114.0', count: 2 }],
          countries: [{ label: 'Unknown', count: 2 }],
          asns: [],
          protocols: [{ label: '/ckb/2', count: 2 }],
        })
      )
    );

    render(<NetworkClientPage />);

    // The dashboard mounts (summary has data) but geo is only "Unknown" => no attribution.
    expect(await screen.findByText('Discovered Reachable')).toBeInTheDocument();
    expect(screen.queryByText(ATTRIBUTION)).not.toBeInTheDocument();
  });
});
