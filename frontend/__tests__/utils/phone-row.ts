import { vi } from 'vitest';
import { stubChartWidth } from '@/__tests__/utils/chart-width';

export interface PhoneLineText {
  key: string | null;
  tone: string | null;
  left: string;
  right: string | null;
}

/** A phone row's lines as text: key, voice, and the left and right parts (see PhoneRow). */
export function phoneLines(row: Element): PhoneLineText[] {
  return Array.from(row.querySelectorAll(':scope > [data-line]')).map((line) => ({
    key: line.getAttribute('data-line'),
    tone: line.getAttribute('data-tone'),
    left: line.querySelector(':scope > [data-part="left"]')?.textContent ?? '',
    right: line.querySelector(':scope > [data-part="right"]')?.textContent ?? null,
  }));
}

/**
 * Lay every list out in a container `width` px wide (jsdom has no layout): 358 is a phone's
 * content width, 608 the 640–767 band's. Restore with vi.unstubAllGlobals + vi.restoreAllMocks.
 */
export function atContainerWidth(width: number): void {
  stubChartWidth(width);
}

/**
 * Charts at `chart` px and every list container at `list` px: a page test that sizes its charts
 * (stubChartWidth) keeps its lists at the page's real content width (1248 at 1280, 608 at 640).
 */
export function stubLayoutWidths({ chart, list }: { chart: number; list: number }): void {
  const widthOf = (el: Element) => (el.hasAttribute('data-list-form') ? list : chart);
  class ResizeObserverStub {
    constructor(private readonly cb: ResizeObserverCallback) {}
    observe(el: Element) {
      this.cb(
        [{ contentRect: { width: widthOf(el) } } as ResizeObserverEntry],
        this as unknown as ResizeObserver
      );
    }
    unobserve() {}
    disconnect() {}
  }
  vi.stubGlobal('ResizeObserver', ResizeObserverStub);
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
    this: HTMLElement
  ) {
    const width = widthOf(this);
    return {
      width,
      height: 0,
      top: 0,
      left: 0,
      right: width,
      bottom: 0,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    } as DOMRect;
  });
}
