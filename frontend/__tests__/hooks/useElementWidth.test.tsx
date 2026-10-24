import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, render, screen } from '@testing-library/react';
import { useElementWidth } from '@/hooks/useElementWidth';

function Probe() {
  const [ref, width] = useElementWidth<HTMLDivElement>();
  return (
    <div ref={ref} data-testid="probe">
      {width === null ? 'unmeasured' : `w=${width}`}
    </div>
  );
}

describe('useElementWidth', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('reads the width before paint and follows resizes', () => {
    let report: ((w: number) => void) | null = null;
    const disconnect = vi.fn();
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(cb: ResizeObserverCallback) {
          report = (w) =>
            cb(
              [{ contentRect: { width: w } } as ResizeObserverEntry],
              this as unknown as ResizeObserver
            );
        }
        observe() {}
        disconnect = disconnect;
      }
    );
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue({
      width: 300,
    } as DOMRect);

    const { unmount } = render(<Probe />);
    expect(screen.getByTestId('probe')).toHaveTextContent('w=300');

    act(() => report?.(412));
    expect(screen.getByTestId('probe')).toHaveTextContent('w=412');

    unmount();
    expect(disconnect).toHaveBeenCalled();
  });

  it('stays unmeasured while the element has no width', () => {
    vi.stubGlobal(
      'ResizeObserver',
      class {
        observe() {}
        disconnect() {}
      }
    );
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue({
      width: 0,
    } as DOMRect);
    render(<Probe />);
    expect(screen.getByTestId('probe')).toHaveTextContent('unmeasured');
  });
});
