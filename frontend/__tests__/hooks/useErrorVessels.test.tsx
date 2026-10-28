import { act, renderHook, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import type { ReactNode } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useErrorVessels, useVesselCaption } from '@/hooks/useErrorVessels';
import { useRealtimeData, useRealtimeStore } from '@/hooks/useRealtimeStore';
import { api, type VesselSnapshot } from '@/lib/api';
import producer from '@/__tests__/fixtures/vessels-api-producer/vessels-snapshot.json';

vi.mock('@/hooks/useRealtimeStore', async (importOriginal) => {
  const original = await importOriginal<typeof import('@/hooks/useRealtimeStore')>();
  return { ...original, useRealtimeData: vi.fn() };
});

const first = producer as unknown as VesselSnapshot;
const second: VesselSnapshot = {
  ...first,
  anchorBlock: 1,
  anchorHash: '0x' + 'ab'.repeat(32),
  cells: [first.cells[1]],
  tracked: [
    {
      outpoint: first.cells[0].outpoint,
      status: 'notCanonical',
      consumedAtBlock: null,
      consumingTxHash: null,
    },
    {
      outpoint: first.cells[1].outpoint,
      status: 'live',
      consumedAtBlock: null,
      consumingTxHash: null,
    },
  ],
};

function wrapper({ children }: { children: ReactNode }) {
  return (
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      {children}
    </QueryClientProvider>
  );
}

describe('useErrorVessels', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    useRealtimeStore.setState({ chainRevision: 0, reorgRevision: 0 });
    vi.mocked(useRealtimeData).mockReturnValue({
      isConnected: true,
      latestBlock: null,
      latestTx: null,
    });
  });

  it('accepts a lower canonical anchor only after a reorg and keeps same-height hash mismatch waiting', async () => {
    const get = vi
      .spyOn(api, 'getVesselSnapshot')
      .mockResolvedValueOnce(first)
      .mockResolvedValueOnce(second);
    const hook = renderHook(() => useErrorVessels(true), { wrapper });
    await waitFor(() => expect(hook.result.current.snapshot?.anchorBlock).toBe(2));
    expect(get).toHaveBeenCalledWith([]);
    vi.mocked(useRealtimeData).mockReturnValue({
      isConnected: true,
      latestBlock: { number: 2, hash: '0x' + 'cd'.repeat(32) } as never,
      latestTx: null,
    });
    hook.rerender();
    await waitFor(() => expect(useVesselCaption.getState().state).toBe('waiting'));
    act(() => useRealtimeStore.setState({ chainRevision: 1, reorgRevision: 1 }));
    await waitFor(() => expect(hook.result.current.snapshot?.anchorBlock).toBe(1));
    expect(hook.result.current.change?.consumed).toEqual([]);
    expect(hook.result.current.change?.rotatedOrReorganized).toEqual([first.cells[0].outpoint]);
    expect(get).toHaveBeenLastCalledWith(first.cells.map((cell) => cell.outpoint));
    hook.unmount();
    expect(useVesselCaption.getState().network).toBeNull();
  });

  it('waits through a reorg when the secondary still returns the old anchor', async () => {
    const sameHeightReplacement = { ...first, anchorHash: '0x' + 'dd'.repeat(32) };
    vi.spyOn(api, 'getVesselSnapshot')
      .mockResolvedValueOnce(first)
      .mockResolvedValueOnce(first)
      .mockResolvedValueOnce(sameHeightReplacement);
    const hook = renderHook(() => useErrorVessels(true), { wrapper });
    await waitFor(() => expect(hook.result.current.snapshot?.anchorHash).toBe(first.anchorHash));
    act(() => useRealtimeStore.setState({ chainRevision: 1, reorgRevision: 1 }));
    await waitFor(() => expect(useVesselCaption.getState().state).toBe('waiting'));
    expect(hook.result.current.snapshot?.anchorHash).toBe(first.anchorHash);
    act(() => useRealtimeStore.setState({ chainRevision: 2, reorgRevision: 1 }));
    await waitFor(() =>
      expect(hook.result.current.snapshot?.anchorHash).toBe(sameHeightReplacement.anchorHash)
    );
    expect(useVesselCaption.getState().state).toBe('live');
  });
});
