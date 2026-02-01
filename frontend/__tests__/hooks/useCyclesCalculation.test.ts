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
}));

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return function Wrapper({ children }: { children: React.ReactNode }) {
    return React.createElement(QueryClientProvider, { client: queryClient }, children);
  };
}

describe('useCyclesCalculation', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.clearAllTimers();
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

    const { result } = renderHook(() => useCyclesCalculation('0xabc', undefined, false), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(api.triggerCyclesCalculation).toHaveBeenCalledWith('0xabc');
    });

    await waitFor(() => {
      expect(result.current.isCalculating).toBe(true);
    });
  });

  it('sets hasFailed when calculation fails', async () => {
    vi.mocked(api.triggerCyclesCalculation).mockResolvedValue({
      status: 'failed',
      cycles: null,
      error: 'Calculation failed',
    });

    const { result } = renderHook(() => useCyclesCalculation('0xabc', undefined, false), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(result.current.hasFailed).toBe(true);
    });

    expect(result.current.isCalculating).toBe(false);
  });

  it('sets hasFailed when not found', async () => {
    vi.mocked(api.triggerCyclesCalculation).mockResolvedValue({
      status: 'notFound',
      cycles: null,
      error: 'Transaction not found',
    });

    const { result } = renderHook(() => useCyclesCalculation('0xabc', undefined, false), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(result.current.hasFailed).toBe(true);
    });
  });

  it('resets state when hash changes', async () => {
    vi.mocked(api.triggerCyclesCalculation).mockResolvedValue({
      status: 'failed',
      cycles: null,
      error: 'Failed',
    });

    const { result, rerender } = renderHook(
      ({ hash }) => useCyclesCalculation(hash, undefined, false),
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

    expect(result.current.hasFailed).toBe(false);
    expect(result.current.isCalculating).toBe(false);
  });
});
