import { afterEach, describe, expect, it, vi } from 'vitest';
import { render } from '@testing-library/react';
import { TitleId } from '@/components/ui/title-id';
import { shortenOutpoint } from '@/lib/shorten';
import { stubChartWidth } from '@/__tests__/utils/chart-width';

/**
 * An id as a page's h1 (U-09; docs/RESPONSIVE_LAYOUT.md principle 4, the lead's ruling of the B3
 * follow-up): the head form where the h1's measured line holds it whole, the row form otherwise,
 * the row form until measured. jsdom lays nothing out: the line is stubbed (stubChartWidth) and so
 * is the h1's computed type, 26 px below md and 30 from md with the h1's −0.02 em tracking
 * (globals.css), so a character advances 0.58 em: 15.08 px at 26 px, 17.4 at 30.
 */
const HASH = `0x${'ab'.repeat(32)}`;
const form = (size: 'head' | 'row') => shortenOutpoint(HASH, 0, size);

function stubType(fontPx: number) {
  const real = window.getComputedStyle.bind(window);
  vi.spyOn(window, 'getComputedStyle').mockImplementation((el, pseudo) => {
    const style = real(el, pseudo);
    if (!(el as Element).hasAttribute('data-title-id')) return style;
    return new Proxy(style, {
      get: (target, prop) =>
        prop === 'fontSize'
          ? `${fontPx}px`
          : prop === 'letterSpacing'
            ? `${-0.02 * fontPx}px`
            : (target as unknown as Record<string | symbol, unknown>)[prop],
    });
  });
}

const shown = (container: HTMLElement) => container.querySelector('[data-title-id]') as HTMLElement;

describe('TitleId: the h1 id by its measured line (U-09)', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it('is the row form until the line is measured', () => {
    // jsdom: no width and no type to read
    const { container } = render(
      <h1>
        <TitleId form={form} />
      </h1>
    );
    expect(shown(container)).toHaveTextContent(form('row'));
    expect(shown(container)).toHaveAttribute('data-title-id', 'row');
  });

  // the head out point, 0xababababab…abababab:0, is 23 characters: 23 × 15.08 = 346.84 px at 26 px
  it.each([
    [346, 'row'],
    [347, 'head'],
    [358, 'head'], // the line at 390
    [328, 'row'], // the line at 360
  ] as const)('at 26 px on a %i px line: the %s form', (line, size) => {
    stubChartWidth(line);
    stubType(26);
    const { container } = render(
      <h1>
        <TitleId form={form} />
      </h1>
    );
    expect(form('head')).toHaveLength(23);
    expect(shown(container)).toHaveTextContent(form(size));
    expect(shown(container)).toHaveAttribute('data-title-id', size);
  });

  it('reads the h1’s own size: at 30 px the head form needs 23 × 17.4 = 400.2 px', () => {
    stubChartWidth(400);
    stubType(30);
    const narrow = render(
      <h1>
        <TitleId form={form} />
      </h1>
    );
    expect(shown(narrow.container)).toHaveAttribute('data-title-id', 'row');
    narrow.unmount();
    vi.restoreAllMocks();
    stubChartWidth(401);
    stubType(30);
    const wide = render(
      <h1>
        <TitleId form={form} />
      </h1>
    );
    expect(shown(wide.container)).toHaveAttribute('data-title-id', 'head');
  });

  it('keeps the form one unbroken unit', () => {
    stubChartWidth(600);
    stubType(26);
    const { container } = render(
      <h1>
        <TitleId form={form} />
      </h1>
    );
    expect(shown(container).querySelector('[data-nobreak]')).toHaveTextContent(form('head'));
  });
});
