import { renderHook, waitFor, act } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import React from 'react';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { useCyclesCalculation } from '@/hooks/useCyclesCalculation';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    triggerCyclesCalculation: vi.fn(),
    getCyclesStatus: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
}));

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return function Wrapper({ children }: { children: React.ReactNode }) {
    return React.createElement(QueryClientProvider, { client: queryClient }, children);
  };
}

function createWrapperWithClient() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const Wrapper = ({ children }: { children: React.ReactNode }) =>
    React.createElement(QueryClientProvider, { client: queryClient }, children);

  return { Wrapper, queryClient };
}

describe('useCyclesCalculation', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.clearAllTimers();
    vi.useRealTimers();
  });

  it('returns cycles when already available', () => {
    const { result } = renderHook(() => useCyclesCalculation('0xabc', 1000, false), {
      wrapper: createWrapper(),
    });

    expect(result.current.cycles).toBe(1000);
    expect(result.current.hasCycles).toBe(true);
    expect(result.current.isCalculating).toBe(false);
    expect(result.current.hasFailed).toBe(false);
  });

  it('does not trigger calculation for cellbase transactions', () => {
    const { result } = renderHook(() => useCyclesCalculation('0xabc', undefined, true), {
      wrapper: createWrapper(),
    });

    expect(result.current.cycles).toBeNull();
    expect(result.current.hasCycles).toBe(false);
    expect(api.triggerCyclesCalculation).not.toHaveBeenCalled();
  });

  it('triggers calculation when cycles are missing', async () => {
    vi.mocked(api.triggerCyclesCalculation).mockResolvedValue({
      status: 'calculating',
      cycles: null,
      error: null,
    });

    const { result } = renderHook(
      () => useCyclesCalculation('0xabc', undefined, false, 'pending'),
      { wrapper: createWrapper() }
    );

    await waitFor(() => {
      expect(api.triggerCyclesCalculation).toHaveBeenCalledWith('0xabc');
    });

    await waitFor(() => {
      expect(result.current.isCalculating).toBe(true);
    });
  });

  it('shows calculating immediately while trigger request is in flight', async () => {
    let resolveTrigger:
      | ((value: { status: string; cycles: number | null; error: string | null }) => void)
      | null = null;
    const pendingTrigger = new Promise<{
      status: string;
      cycles: number | null;
      error: string | null;
    }>((resolve) => {
      resolveTrigger = resolve;
    });
    vi.mocked(api.triggerCyclesCalculation).mockReturnValue(
      pendingTrigger as Promise<{ status: 'done'; cycles: number; error: null }>
    );

    const { result } = renderHook(
      () => useCyclesCalculation('0xabc', undefined, false, 'pending'),
      { wrapper: createWrapper() }
    );

    await waitFor(() => {
      expect(api.triggerCyclesCalculation).toHaveBeenCalledWith('0xabc');
      expect(result.current.isCalculating).toBe(true);
    });

    (
      resolveTrigger as unknown as (value: {
        status: string;
        cycles: number | null;
        error: string | null;
      }) => void
    )({
      status: 'done',
      cycles: 1000,
      error: null,
    });

    await waitFor(() => {
      expect(result.current.cycles).toBe(1000);
      expect(result.current.isCalculating).toBe(false);
    });
  });

  it('keeps calculating when trigger returns done without cycles', async () => {
    vi.mocked(api.triggerCyclesCalculation).mockResolvedValue({
      status: 'done',
      cycles: null,
      error: null,
    });

    const { result } = renderHook(
      () => useCyclesCalculation('0xabc', undefined, false, 'pending'),
      { wrapper: createWrapper() }
    );

    await waitFor(() => {
      expect(api.triggerCyclesCalculation).toHaveBeenCalledWith('0xabc');
      expect(result.current.isCalculating).toBe(true);
      expect(result.current.hasCycles).toBe(false);
      expect(result.current.hasFailed).toBe(false);
    });
  });

  it('uses cycles when trigger returns done with cycles', async () => {
    vi.mocked(api.triggerCyclesCalculation).mockResolvedValue({
      status: 'done',
      cycles: 123456,
      error: null,
    });

    const { result } = renderHook(
      () => useCyclesCalculation('0xabc', undefined, false, 'pending'),
      { wrapper: createWrapper() }
    );

    await waitFor(() => {
      expect(api.triggerCyclesCalculation).toHaveBeenCalledWith('0xabc');
    });

    await waitFor(() => {
      expect(result.current.cycles).toBe(123456);
      expect(result.current.hasCycles).toBe(true);
      expect(result.current.isCalculating).toBe(false);
    });
  });

  it('uses cycles when polling returns done with cycles', async () => {
    vi.mocked(api.triggerCyclesCalculation).mockResolvedValue({
      status: 'calculating',
      cycles: null,
      error: null,
    });
    vi.mocked(api.getCyclesStatus).mockResolvedValue({
      status: 'done',
      cycles: 777,
      error: null,
    });
    const setIntervalSpy = vi.spyOn(global, 'setInterval').mockImplementation((handler) => {
      if (typeof handler === 'function') {
        void handler();
      }
      return 1 as unknown as ReturnType<typeof setInterval>;
    });
    const clearIntervalSpy = vi.spyOn(global, 'clearInterval').mockImplementation(() => {});

    const { result } = renderHook(
      () => useCyclesCalculation('0xabc', undefined, false, 'pending'),
      { wrapper: createWrapper() }
    );

    await waitFor(() => {
      expect(api.triggerCyclesCalculation).toHaveBeenCalledWith('0xabc');
    });

    await waitFor(() => expect(api.getCyclesStatus).toHaveBeenCalledWith('0xabc'));
    await waitFor(() => expect(result.current.cycles).toBe(777));
    expect(result.current.hasCycles).toBe(true);
    expect(result.current.isCalculating).toBe(false);

    setIntervalSpy.mockRestore();
    clearIntervalSpy.mockRestore();
  });

  it('invalidates transaction query while polling remains calculating', async () => {
    vi.mocked(api.triggerCyclesCalculation).mockResolvedValue({
      status: 'calculating',
      cycles: null,
      error: null,
    });
    vi.mocked(api.getCyclesStatus).mockResolvedValue({
      status: 'calculating',
      cycles: null,
      error: null,
    });
    const setIntervalSpy = vi.spyOn(global, 'setInterval').mockImplementation((handler) => {
      if (typeof handler === 'function') {
        void handler();
      }
      return 1 as unknown as ReturnType<typeof setInterval>;
    });
    const clearIntervalSpy = vi.spyOn(global, 'clearInterval').mockImplementation(() => {});
    const { Wrapper, queryClient } = createWrapperWithClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');

    const { result } = renderHook(
      () => useCyclesCalculation('0xabc', undefined, false, 'pending'),
      { wrapper: Wrapper }
    );

    await waitFor(() => expect(api.getCyclesStatus).toHaveBeenCalledWith('0xabc'));
    await waitFor(() =>
      expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: ['transaction', '0xabc'] })
    );
    expect(result.current.isCalculating).toBe(true);

    setIntervalSpy.mockRestore();
    clearIntervalSpy.mockRestore();
  });

  it('sets hasFailed when calculation fails', async () => {
    vi.mocked(api.triggerCyclesCalculation).mockResolvedValue({
      status: 'failed',
      cycles: null,
      error: 'Calculation failed',
    });

    const { result } = renderHook(
      () => useCyclesCalculation('0xabc', undefined, false, 'pending'),
      { wrapper: createWrapper() }
    );

    await waitFor(() => {
      expect(result.current.hasFailed).toBe(true);
    });

    expect(result.current.isCalculating).toBe(false);
  });

  it('keeps polling when not found (tx not yet indexed)', async () => {
    vi.mocked(api.triggerCyclesCalculation).mockResolvedValue({
      status: 'notFound',
      cycles: null,
      error: 'Transaction not found',
    });

    const { result } = renderHook(
      () => useCyclesCalculation('0xabc', undefined, false, 'pending'),
      { wrapper: createWrapper() }
    );

    await waitFor(() => {
      expect(api.triggerCyclesCalculation).toHaveBeenCalled();
    });
    // notFound keeps calculating (polling) — not a permanent failure
    expect(result.current.hasFailed).toBe(false);
    expect(result.current.isCalculating).toBe(true);
  });

  it('sets hasFailed on network error during trigger', async () => {
    vi.mocked(api.triggerCyclesCalculation).mockRejectedValue(new Error('Network error'));

    const { result } = renderHook(
      () => useCyclesCalculation('0xabc', undefined, false, 'pending'),
      { wrapper: createWrapper() }
    );

    await waitFor(() => {
      expect(result.current.hasFailed).toBe(true);
    });
    expect(result.current.isCalculating).toBe(false);
  });

  it('resets state when hash changes', async () => {
    vi.mocked(api.triggerCyclesCalculation).mockResolvedValue({
      status: 'failed',
      cycles: null,
      error: 'Failed',
    });

    const { result, rerender } = renderHook(
      ({ hash }) => useCyclesCalculation(hash, undefined, false, 'pending'),
      { wrapper: createWrapper(), initialProps: { hash: '0xabc' } }
    );

    await waitFor(() => {
      expect(result.current.hasFailed).toBe(true);
    });

    vi.mocked(api.triggerCyclesCalculation).mockResolvedValue({
      status: 'done',
      cycles: 1000,
      error: null,
    });

    act(() => {
      rerender({ hash: '0xdef' });
    });

    await waitFor(() => {
      expect(result.current.hasFailed).toBe(false);
      expect(result.current.isCalculating).toBe(true);
    });

    await waitFor(() => {
      expect(result.current.cycles).toBe(1000);
      expect(result.current.isCalculating).toBe(false);
    });
  });
});
