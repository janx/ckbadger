import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, render, renderHook, waitFor } from '@testing-library/react';
import { http, HttpResponse } from 'msw';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import type { ReactNode } from 'react';
import { server } from '@/__tests__/msw/server';
import { pipelineSnapshot } from '@/__tests__/msw/fixtures/pipeline-snapshot';
import { createTestQueryClient } from '@/__tests__/utils/test-utils';
import { usePipelineSnapshot } from '@/hooks/usePipelineSnapshot';
import { useRealtimeData, useRealtimeStore } from '@/hooks/useRealtimeStore';

class MockWebSocket {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSING = 2;
  static readonly CLOSED = 3;
  url: string;
  readyState = MockWebSocket.CONNECTING;
  sent: string[] = [];
  onopen: (() => void) | null = null;
  onmessage: ((event: { data: string }) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  constructor(url: string) {
    this.url = url;
    sockets.push(this);
  }
  send(data: string) {
    this.sent.push(data);
  }
  close() {
    this.readyState = MockWebSocket.CLOSED;
  }
}

let sockets: MockWebSocket[] = [];

function wrapperFor(client: QueryClient) {
  return function Wrapper({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
  };
}

describe('usePipelineSnapshot', () => {
  it('returns the /pipeline snapshot', async () => {
    const client = createTestQueryClient();
    const { result } = renderHook(() => usePipelineSnapshot(), { wrapper: wrapperFor(client) });

    await waitFor(() => expect(result.current.data).toBeDefined());
    expect(result.current.data).toEqual(pipelineSnapshot);
    expect(client.getQueryData(['pipeline'])).toEqual(pipelineSnapshot);
  });

  it('polls the snapshot every 2 s', async () => {
    let calls = 0;
    server.use(
      http.get('/api/:network/v1/pipeline', () => {
        calls++;
        return HttpResponse.json(pipelineSnapshot);
      })
    );
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const client = createTestQueryClient();
      renderHook(() => usePipelineSnapshot(), { wrapper: wrapperFor(client) });
      await waitFor(() => expect(calls).toBe(1));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(2000);
      });
      await waitFor(() => expect(calls).toBe(2));
    } finally {
      vi.useRealTimers();
    }
  });

  it('surfaces a backend error instead of an empty snapshot', async () => {
    server.use(
      http.get('/api/:network/v1/pipeline', () =>
        HttpResponse.json({ error: 'pipeline needs ckb_db_path' }, { status: 500 })
      )
    );
    const client = createTestQueryClient();
    const { result } = renderHook(() => usePipelineSnapshot(), { wrapper: wrapperFor(client) });

    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(result.current.data).toBeUndefined();
  });
});

describe('pipeline invalidation from the realtime socket', () => {
  let client: QueryClient;

  function Harness() {
    useRealtimeData();
    return null;
  }

  beforeEach(() => {
    sockets = [];
    vi.stubGlobal('WebSocket', MockWebSocket as unknown as typeof WebSocket);
    useRealtimeStore.setState({ isConnected: false, latestBlock: null, latestTx: null });
    client = new QueryClient();
    client.setQueryData(['pipeline'], pipelineSnapshot);
    render(
      <QueryClientProvider client={client}>
        <MemoryRouter initialEntries={['/mainnet/']}>
          <Routes>
            <Route path="/:network/*" element={<Harness />} />
          </Routes>
        </MemoryRouter>
      </QueryClientProvider>
    );
    act(() => {
      sockets[0].readyState = MockWebSocket.OPEN;
      sockets[0].onopen?.();
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  const send = (type: string, data: unknown) =>
    act(() => {
      sockets[0].onmessage?.({ data: JSON.stringify({ type, data }) });
    });

  it('subscribes to the reorg channel', () => {
    const channels = sockets[0].sent.map((m) => (JSON.parse(m) as { channel: string }).channel);
    expect(channels).toContain('new_block');
    expect(channels).toContain('reorg');
  });

  it('invalidates the snapshot when a block arrives', () => {
    expect(client.getQueryState(['pipeline'])?.isInvalidated).toBe(false);
    send('new_block', {
      number: 18_734_173,
      hash: '0xabc',
      timestamp: '2026-10-23T08:00:08Z',
      transactionsCount: 3,
      epochNumber: 1,
      epochIndex: 2,
      epochLength: 1800,
    });
    expect(client.getQueryState(['pipeline'])?.isInvalidated).toBe(true);
  });

  it('invalidates the snapshot on a reorg', () => {
    expect(client.getQueryState(['pipeline'])?.isInvalidated).toBe(false);
    send('reorg', {
      depth: 1,
      oldTip: 18_734_172,
      newTip: 18_734_172,
      forkPoint: 18_734_171,
      orphanedBlocks: 1,
      orphanedTxs: 2,
      timestamp: '2026-10-23T08:00:08Z',
    });
    expect(client.getQueryState(['pipeline'])?.isInvalidated).toBe(true);
  });
});
