import { useRef } from 'react';
import { act } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { useNight } from '@/hooks/useNight';
import { useRealtimeStore, type Block } from '@/hooks/useRealtimeStore';
import { createNight, type NightOptions } from '@/lib/night/night';

vi.mock('@/lib/night/night', () => ({ createNight: vi.fn() }));
// The feed itself (the socket, which resets the store when it connects) is not under test: the
// hook reads the store, as the real feed fills it.
vi.mock('@/hooks/useRealtimeStore', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/hooks/useRealtimeStore')>();
  return {
    ...actual,
    useRealtimeData: () => {
      const { isConnected, latestBlock, latestTx } = actual.useRealtimeStore();
      return { isConnected, latestBlock, latestTx };
    },
  };
});

const HASH_A = '0x' + 'a'.repeat(64);
const HASH_B = '0x' + 'b'.repeat(64);
const HASH_C = '0x' + 'c'.repeat(64);

function block(hash: string, number = 20_914_858): Block {
  return {
    number,
    hash,
    timestamp: '0',
    transactionsCount: 1,
    epochNumber: 15_228,
    epochIndex: 521,
    epochLength: 1407,
    syncStatus: {
      isSyncing: false,
      syncedBlock: number,
      tipBlock: number,
      progress: 100,
      estimatedTime: null,
      chartDataMayBeIncomplete: false,
      blocksPerSecond: null,
      emaBlocksPerSecond: null,
      syncMode: 'live',
      startedAt: null,
      elapsedTime: null,
      totalTime: null,
    },
  };
}

function Probe({ hash }: { hash: string | null }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const { state, block: latest } = useNight(ref, hash);
  return (
    <>
      <canvas ref={ref} />
      <span data-testid="state">{state}</span>
      <span data-testid="block">{latest?.hash ?? ''}</span>
    </>
  );
}

describe('useNight', () => {
  const pour = vi.fn();
  const stop = vi.fn();
  let lastOptions: NightOptions | null = null;

  beforeEach(() => {
    lastOptions = null;
    vi.mocked(createNight).mockImplementation((_canvas, options) => {
      lastOptions = options;
      options.onState?.('running');
      return { pour, stop };
    });
    useRealtimeStore.getState().reset();
  });
  afterEach(() => {
    vi.clearAllMocks();
    useRealtimeStore.getState().reset();
  });

  it('pours the page’s hash first, then every new block once, and stops with the page', () => {
    const { unmount } = render(<Probe hash={HASH_A} />);
    expect(createNight).toHaveBeenCalledTimes(1);
    expect(lastOptions).toMatchObject({
      hash: HASH_A,
      grid: { cx: 0.72, cy: 0.5, pitch: 18 },
      half: false,
      still: false,
    });
    expect(screen.getByTestId('state')).toHaveTextContent('running');
    expect(pour).not.toHaveBeenCalled();
    act(() => useRealtimeStore.setState({ latestBlock: block(HASH_B) }));
    expect(pour).toHaveBeenCalledWith(HASH_B);
    expect(screen.getByTestId('block')).toHaveTextContent(HASH_B);
    act(() => useRealtimeStore.setState({ latestBlock: block(HASH_B, 20_914_859) }));
    expect(pour).toHaveBeenCalledTimes(1);
    act(() => useRealtimeStore.setState({ latestBlock: block(HASH_C, 20_914_860) }));
    expect(pour).toHaveBeenCalledTimes(2);
    expect(pour).toHaveBeenLastCalledWith(HASH_C);
    unmount();
    expect(stop).toHaveBeenCalledTimes(1);
  });

  it('starts from the tip the feed already knows (another page kept the socket) when the page has no hash of its own', () => {
    useRealtimeStore.setState({ latestBlock: block(HASH_B) });
    render(<Probe hash={null} />);
    expect(lastOptions?.hash).toBe(HASH_B);
    expect(pour).not.toHaveBeenCalled();
    act(() => useRealtimeStore.setState({ latestBlock: block(HASH_C, 20_914_859) }));
    expect(pour).toHaveBeenCalledWith(HASH_C);
  });

  it('waits for the first block when nothing is known, and reports a night that could not start', () => {
    vi.mocked(createNight).mockImplementation((_canvas, options) => {
      lastOptions = options;
      options.onState?.('failed');
      return { pour, stop };
    });
    render(<Probe hash={null} />);
    expect(lastOptions?.hash).toBeNull();
    expect(screen.getByTestId('state')).toHaveTextContent('failed');
  });

  it('asks for a still night under reduced motion', () => {
    const matchMedia = window.matchMedia;
    window.matchMedia = ((query: string) =>
      ({
        matches: query === '(prefers-reduced-motion: reduce)',
        media: query,
        onchange: null,
        addEventListener: () => {},
        removeEventListener: () => {},
        addListener: () => {},
        removeListener: () => {},
        dispatchEvent: () => false,
      }) as MediaQueryList) as typeof window.matchMedia;
    try {
      render(<Probe hash={HASH_A} />);
      expect(lastOptions?.still).toBe(true);
    } finally {
      window.matchMedia = matchMedia;
    }
  });
});
