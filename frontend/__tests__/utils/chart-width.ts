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

/**
 * jsdom has no PointerEvent, so `fireEvent.pointerMove(el, { clientX, pointerType })` would deliver
 * a bare Event with neither. A MouseEvent subclass carrying `pointerType` is enough for React.
 */
export function stubPointerEvents(): void {
  class PointerEventStub extends MouseEvent {
    readonly pointerType: string;
    constructor(type: string, init: PointerEventInit = {}) {
      super(type, init);
      this.pointerType = init.pointerType ?? 'mouse';
    }
  }
  vi.stubGlobal('PointerEvent', PointerEventStub);
}
