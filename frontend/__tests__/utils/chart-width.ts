import { vi } from 'vitest';

/**
 * jsdom has no layout and no ResizeObserver. Give every element `width` px and a ResizeObserver
 * that reports it, so `useElementWidth` (and the homepage charts drawn with it) render.
 */
export function stubChartWidth(width = 280): void {
  class ResizeObserverStub {
    constructor(private readonly cb: ResizeObserverCallback) {}
    observe() {
      this.cb(
        [{ contentRect: { width } } as ResizeObserverEntry],
        this as unknown as ResizeObserver
      );
    }
    unobserve() {}
    disconnect() {}
  }
  vi.stubGlobal('ResizeObserver', ResizeObserverStub);
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue({
    width,
    height: 0,
    top: 0,
    left: 0,
    right: width,
    bottom: 0,
    x: 0,
    y: 0,
    toJSON: () => ({}),
  } as DOMRect);
}
