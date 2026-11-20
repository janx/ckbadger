import { vi } from 'vitest';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { contentPx, gapPx } from '@/__tests__/utils/tailwind-box';
import { ROW_SEAL } from '@/components/ui/row-seal';
import type { SealKind } from '@/lib/seal/stones';

/**
 * A phone row's first line as drawn at a viewport, measured from its classes (jsdom lays nothing
 * out; tailwind-box reads each part at max-content): the stone's footprint where the row leads with
 * one (ROW_SEAL, by the seal's kind), the left part, the line's gap and the right part. What the
 * board's head() sums, read off the row itself.
 */
export function phoneHeadPx(row: Element, viewport: number): number {
  const line = row.querySelector(':scope > [data-line]');
  if (!line) throw new Error('phone-row: the row has no line');
  const left = line.querySelector(':scope > [data-part="left"]');
  const right = line.querySelector(':scope > [data-part="right"]');
  if (!left) throw new Error('phone-row: the first line has no left part');
  const seal = row.querySelector(':scope > [data-part="mark"] [data-seal]');
  const mark = seal ? ROW_SEAL[seal.getAttribute('data-seal') as SealKind].footprint : 0;
  // every part at its natural width: a unit capped at its line (max-w-full) is its content
  const read = { capped: true };
  const px =
    mark +
    contentPx(left, viewport, read) +
    (right ? gapPx(line, viewport) + contentPx(right, viewport, read) : 0);
  return Math.round(px * 100) / 100;
}

export interface PhoneLineText {
  key: string | null;
  tone: string | null;
  left: string;
  right: string | null;
}

/**
 * The first line the row's list states for it (PhoneRow's `head`, `data-head-px`): its widest real
 * line, which every row draws within. A row with no stated line is refused.
 */
export function statedHeadPx(row: Element): number {
  const stated = row.getAttribute('data-head-px');
  if (stated === null) throw new Error('phone-row: the row states no first line (head)');
  return Number(stated);
}

/**
 * The items of the separated lists inside an element (`Dotted`), each as its words without the
 * separator hung before it: what a line holds, item by item, as a reader meets them.
 */
export function dottedItems(el: Element): string[] {
  return Array.from(el.querySelectorAll('[data-dotted-item]')).map((item) => {
    const copy = item.cloneNode(true) as Element;
    copy.querySelectorAll('[data-dotted-separator]').forEach((sep) => sep.remove());
    return (copy.textContent ?? '').trim();
  });
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
