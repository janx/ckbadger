import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter, Route, Routes, useNavigate } from 'react-router-dom';
import { useRealtimeData, useRealtimeStore } from '@/hooks/useRealtimeStore';

// A minimal WebSocket stand-in that records every socket the store opens, so we
// can assert which network URL it targets and that the old one is torn down.
class MockWebSocket {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSING = 2;
  static readonly CLOSED = 3;
  url: string;
  readyState = MockWebSocket.CONNECTING;
  closed = false;
  onopen: (() => void) | null = null;
  onmessage: ((event: { data: string }) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  constructor(url: string) {
    this.url = url;
    instances.push(this);
  }
  send() {}
  close() {
    this.closed = true;
    this.readyState = MockWebSocket.CLOSED;
    // Browsers deliver `close` ASYNCHRONOUSLY: the handler runs on a later task,
    // long after close() returned. Firing it synchronously would hide teardown
    // races (a late onclose resurrecting the switched-away network's socket).
    setTimeout(() => this.onclose?.(), 0);
  }
}

let instances: MockWebSocket[] = [];

// Mirrors RECONNECT_INTERVAL in @/hooks/useRealtimeStore.
const RECONNECT_INTERVAL_MS = 3000;

function Harness() {
  useRealtimeData();
  const navigate = useNavigate();
  return (
    <button type="button" onClick={() => navigate('/testnet/')}>
      go-testnet
    </button>
  );
}

function renderHarness(initialPath = '/mainnet/') {
  const queryClient = new QueryClient();
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[initialPath]}>
        <Routes>
          <Route path="/:network/*" element={<Harness />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>
  );
}

describe('useRealtimeData network scoping', () => {
  beforeEach(() => {
    instances = [];
    vi.stubGlobal('WebSocket', MockWebSocket as unknown as typeof WebSocket);
    window.__CKBADGER_RUNTIME_CONFIG__ = {
      networks: [{ name: 'mainnet' }, { name: 'testnet' }],
      defaultNetwork: 'mainnet',
    };
    useRealtimeStore.setState({ isConnected: false, latestBlock: null, latestTx: null });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    delete window.__CKBADGER_RUNTIME_CONFIG__;
  });

  it('opens the active network socket and reconnects to the new network on switch', async () => {
    const user = userEvent.setup();
    renderHarness();

    expect(instances).toHaveLength(1);
    expect(instances[0].url).toMatch(/\/ws\/mainnet$/);

    await user.click(screen.getByRole('button', { name: 'go-testnet' }));

    // A new socket targeting testnet was opened, and the mainnet socket was closed.
    const latest = instances[instances.length - 1];
    expect(instances.length).toBeGreaterThanOrEqual(2);
    expect(latest.url).toMatch(/\/ws\/testnet$/);
    expect(instances[0].closed).toBe(true);
  });

  it('never reconnects to the switched-away network when the old socket closes late', async () => {
    vi.useFakeTimers();
    try {
      // NetworkQueryScope keys its subtree by the active network, so a switch
      // unmounts the old network's subtree (disconnect) before mounting the new
      // one (connect).
      const mainnetView = renderHarness('/mainnet/');
      expect(instances).toHaveLength(1);
      expect(instances[0].url).toMatch(/\/ws\/mainnet$/);

      const openedBeforeSwitch = instances.length;

      mainnetView.unmount();
      renderHarness('/testnet/');

      // The mainnet socket's close event lands only now — after the testnet
      // socket already exists — and any reconnect it schedules fires later still.
      await act(async () => {
        await vi.advanceTimersByTimeAsync(RECONNECT_INTERVAL_MS * 2);
      });

      const openedAfterSwitch = instances.slice(openedBeforeSwitch).map((ws) => ws.url);
      expect(openedAfterSwitch.length).toBeGreaterThan(0);
      // Every socket opened after the switch must target testnet: a late mainnet
      // reconnect would stream the wrong network's blocks into the new cache.
      expect(openedAfterSwitch.filter((url) => /\/ws\/mainnet$/.test(url))).toEqual([]);
      // Exactly one live socket remains — no orphaned socket still feeding handlers.
      expect(instances.filter((ws) => !ws.closed)).toHaveLength(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it('resets stale realtime store data on network switch', async () => {
    const user = userEvent.setup();
    renderHarness();

    // Simulate a mainnet block having populated the shared store.
    act(() => {
      useRealtimeStore.setState({
        isConnected: true,
        latestBlock: {
          number: 999,
          hash: '0xmainnet',
          timestamp: '2024-01-01T00:00:00Z',
          transactionsCount: 1,
        } as never,
      });
    });
    expect(useRealtimeStore.getState().latestBlock).not.toBeNull();

    await user.click(screen.getByRole('button', { name: 'go-testnet' }));

    // The switched-away network's data must not linger.
    expect(useRealtimeStore.getState().latestBlock).toBeNull();
    expect(useRealtimeStore.getState().isConnected).toBe(false);
  });
});
