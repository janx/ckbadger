import { act, render, screen } from '@/__tests__/utils/test-utils';
import { describe, expect, it, vi } from 'vitest';
import { ErrorVessels, VesselsCaption } from '@/components/error-vessels';
import { useErrorVessels, useVesselCaption } from '@/hooks/useErrorVessels';
import producer from '@/__tests__/fixtures/vessels-api-producer/vessels-snapshot.json';
import type { VesselSnapshot } from '@/lib/api';

vi.mock('@/hooks/useErrorVessels', async (importOriginal) => {
  const original = await importOriginal<typeof import('@/hooks/useErrorVessels')>();
  return { ...original, useErrorVessels: vi.fn() };
});

describe('ErrorVessels', () => {
  it("does not show another network's caption before its own snapshot loads", () => {
    act(() =>
      useVesselCaption.setState({
        network: 'testnet',
        state: 'live',
        anchorBlock: 100,
        visible: 3,
        blocksInWindow: 12,
        message: null,
      })
    );
    render(<VesselsCaption />);
    expect(screen.getByText(/Reading a bounded sample/)).toBeInTheDocument();
    expect(screen.queryByText(/block #100/)).not.toBeInTheDocument();
  });

  it('draws producer-backed anatomy and captions a bounded, anchored sample', () => {
    const fillRect = vi.fn();
    const context = {
      setTransform: vi.fn(),
      clearRect: vi.fn(),
      beginPath: vi.fn(),
      roundRect: vi.fn(),
      fill: vi.fn(),
      stroke: vi.fn(),
      fillRect,
      set fillStyle(_value: string) {},
      set strokeStyle(_value: string) {},
      set globalAlpha(_value: number) {},
      set lineWidth(_value: number) {},
    };
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(
      context as unknown as CanvasRenderingContext2D
    );
    vi.spyOn(window, 'requestAnimationFrame').mockImplementation((callback) => {
      callback(1000);
      return 1;
    });
    vi.spyOn(window, 'cancelAnimationFrame').mockImplementation(() => {});
    vi.stubGlobal('matchMedia', () => ({ matches: true }));
    vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(800);
    vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(400);
    vi.mocked(useErrorVessels).mockReturnValue({
      snapshot: producer as unknown as VesselSnapshot,
      change: null,
      isConnected: true,
    });
    act(() =>
      useVesselCaption.setState({
        network: 'mainnet',
        state: 'live',
        anchorBlock: 2,
        visible: 2,
        blocksInWindow: 3,
        message: null,
      })
    );
    const view = render(
      <>
        <ErrorVessels />
        <VesselsCaption />
      </>
    );
    expect(
      screen.getByText(
        /2 live Common knowledge cells from mainnet: first 6 transactions and first 4 outputs/
      )
    ).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'block #2' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/2'
    );
    expect(screen.getByText(/diagram area is illustrative/)).toBeInTheDocument();
    expect(fillRect).toHaveBeenCalled();
    expect(fillRect.mock.calls.length).toBeGreaterThanOrEqual(6);
    const clearsBeforeInvalidation = context.clearRect.mock.calls.length;
    vi.mocked(useErrorVessels).mockReturnValue({
      snapshot: null,
      change: null,
      isConnected: false,
    });
    view.rerender(
      <>
        <ErrorVessels />
        <VesselsCaption />
      </>
    );
    expect(context.clearRect.mock.calls.length).toBeGreaterThan(clearsBeforeInvalidation);
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });
});
