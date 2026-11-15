import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { HeadId } from '@/components/ui/head-id';
import { stubChartWidth } from '@/__tests__/utils/chart-width';

/**
 * A full id in a page's head (the lead's ruling a, 2026-11-14, widened to every head by the A4
 * Chrome findings, F3): never a sideways scroller (RESPONSIVE_LAYOUT principle 1, the first
 * screen) and never broken where the browser pleases. It sits on ONE line where the measured line
 * holds it, else in equal designed lines, each one unit:
 *
 * - a hex id: `0x` and the first part of the digits, the rest in lines of as many digits indented
 *   2ch, so the digits stand under the digits — a 66-character hash is the name plate's two
 *   halves, 34 + 32;
 * - an address: as few lines of equal length as hold it (the last the remainder).
 *
 * The advance is JetBrains Mono's 0.6 em of the line's computed size plus its tracking, the head's
 * voice (jsdom computes no type, so it is stubbed): 7.2 px at a plate's 12 px, 6.6 px at an asset
 * head's 11 px. A hash takes 66 × 7.2 = 475.2 px on one line at 12 px (435.6 at
 * 11 px); a 97-character address 698.4 px; a 620-character address (a Godwoken lock) 4,464 px.
 * Moved from the PlateHash test, its every case kept.
 */
const HASH = `0x${'0123456789abcdef'.repeat(4)}`;
const BECH32 = 'qpzry9x8gf2tvdw0s3jn54khce6mua7l';
const address = (length: number) =>
  `ckb1${Array.from({ length: length - 4 }, (_, i) => BECH32[(i * 7) % 32]).join('')}`;
const ADDRESS = address(97);
const LONG_ADDRESS = address(620);

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

/** The line's width (stubChartWidth) and its type, 12 px by default, untracked. */
function stubLine(width: number, fontPx = 12) {
  stubChartWidth(width);
  const real = window.getComputedStyle.bind(window);
  vi.spyOn(window, 'getComputedStyle').mockImplementation((el, pseudo) => {
    const style = real(el, pseudo);
    if (!(el as Element).hasAttribute('data-head-id')) return style;
    return new Proxy(style, {
      get: (target, prop) =>
        prop === 'fontSize'
          ? `${fontPx}px`
          : prop === 'letterSpacing'
            ? 'normal'
            : (target as unknown as Record<string | symbol, unknown>)[prop],
    });
  });
}

const head = () => document.querySelector('[data-head-id]') as HTMLElement;
const lines = () => [...head().querySelectorAll<HTMLElement>('[data-head-line]')];
const texts = () => lines().map((line) => line.textContent);

/** Every line one unit, a <br> between lines, the whole id once in the text and the title. */
function expectDesigned(value: string) {
  for (const line of lines()) expect(line).toHaveClass('whitespace-nowrap');
  lines()
    .slice(0, -1)
    .forEach((line) => expect(line.nextElementSibling?.tagName).toBe('BR'));
  expect(head().textContent).toBe(value);
  expect(head()).toHaveAttribute('title', value);
  expect(head().querySelector('.overflow-x-auto')).toBeNull();
}

describe('HeadId, a full id in a page head', () => {
  it('sets a hash in two halves where the line cannot hold it (358 px, a 390 phone)', () => {
    stubLine(358);
    render(<HeadId value={HASH} kind="hash" />);
    const [first, second] = lines();
    expect(texts()).toEqual([HASH.slice(0, 34), HASH.slice(34)]);
    // the second stands 2ch in, its digits under the first's
    expect(first).not.toHaveClass('pl-[2ch]');
    expect(second).toHaveClass('pl-[2ch]');
    expectDesigned(HASH);
  });

  it('keeps a hash on one line where the line holds it, split just below (12 px: 475.2)', () => {
    stubLine(476);
    const { unmount } = render(<HeadId value={HASH} kind="hash" />);
    expect(texts()).toEqual([HASH]);
    expect(head().querySelector('br')).toBeNull();
    unmount();
    vi.restoreAllMocks();
    stubLine(475);
    render(<HeadId value={HASH} kind="hash" />);
    expect(lines()).toHaveLength(2);
  });

  it('measures at its line’s type: 11 px holds a hash from 435.6 px', () => {
    stubLine(436, 11);
    const { unmount } = render(<HeadId value={HASH} kind="hash" />);
    expect(texts()).toEqual([HASH]);
    // it sets no voice of its own: the head's line does
    expect(head().className).not.toMatch(/(^|\s)text-/);
    unmount();
    vi.restoreAllMocks();
    stubLine(435, 11);
    render(<HeadId value={HASH} kind="hash" />);
    expect(texts()).toEqual([HASH.slice(0, 34), HASH.slice(34)]);
  });

  it('takes more equal lines where half a hash does not fit (240 px at 12 px: 33 characters)', () => {
    stubLine(240);
    render(<HeadId value={HASH} kind="hash" />);
    // 0x and 22 digits, then 22 and 20, each later line 2ch in
    expect(texts()).toEqual([HASH.slice(0, 24), HASH.slice(24, 46), HASH.slice(46)]);
    expectDesigned(HASH);
  });

  it('sets a 97-character address in as few equal lines as hold it', () => {
    stubLine(699);
    const { unmount } = render(<HeadId value={ADDRESS} kind="address" />);
    expect(texts()).toEqual([ADDRESS]);
    unmount();
    // 698 px: 96 characters a line, two lines of 49 and 48 (the address page at 640: 578 px)
    for (const width of [698, 578]) {
      vi.restoreAllMocks();
      stubLine(width);
      const view = render(<HeadId value={ADDRESS} kind="address" />);
      expect(texts()).toEqual([ADDRESS.slice(0, 49), ADDRESS.slice(49)]);
      expectDesigned(ADDRESS);
      view.unmount();
    }
    // 328 px, the address page at 390 beside its copy button: 45 a line, three of 33, 33 and 31
    vi.restoreAllMocks();
    stubLine(328);
    render(<HeadId value={ADDRESS} kind="address" />);
    expect(texts()).toEqual([ADDRESS.slice(0, 33), ADDRESS.slice(33, 66), ADDRESS.slice(66)]);
    // an address has no prefix to stand under: no line is indented
    for (const line of lines()) expect(line).not.toHaveClass('pl-[2ch]');
    expectDesigned(ADDRESS);
  });

  it('sets a 620-character address in as many equal lines as it needs', () => {
    stubLine(4464);
    const { unmount } = render(<HeadId value={LONG_ADDRESS} kind="address" />);
    expect(texts()).toEqual([LONG_ADDRESS]);
    unmount();
    vi.restoreAllMocks();
    stubLine(4463);
    const two = render(<HeadId value={LONG_ADDRESS} kind="address" />);
    expect(texts()).toEqual([LONG_ADDRESS.slice(0, 310), LONG_ADDRESS.slice(310)]);
    two.unmount();
    // 328 px: 45 a line, fourteen lines, thirteen of 45 and the last 35
    vi.restoreAllMocks();
    stubLine(328);
    render(<HeadId value={LONG_ADDRESS} kind="address" />);
    expect(texts().map((t) => t!.length)).toEqual([...Array(13).fill(45), 35]);
    expectDesigned(LONG_ADDRESS);
  });

  it('is one line until it is measured', () => {
    // jsdom lays nothing out: no width, so no measurement (the layout effect measures before the
    // first paint in a browser, so the one-line form is never painted where it does not fit)
    render(<HeadId value={ADDRESS} kind="address" />);
    expect(texts()).toEqual([ADDRESS]);
  });

  it('copies the whole id when it copies, keeping its lines', async () => {
    stubLine(326);
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    render(<HeadId value={HASH} kind="hash" copyable />);
    fireEvent.click(head());
    expect(writeText).toHaveBeenCalledWith(HASH);
    expect(screen.getByTitle(HASH)).toBe(head());
    // the ✓ stands over the id, which keeps its lines (hidden), so the page under it never moves
    await waitFor(() => expect(screen.getByText('✓')).toBeInTheDocument());
    expect(lines()).toHaveLength(2);
    expect(lines()[0].parentElement).toHaveClass('invisible');
  });

  it('refuses a value that is not of its kind', () => {
    expect(() => render(<HeadId value="0x12g4" kind="hash" />)).toThrow('not a hex id');
    expect(() => render(<HeadId value={HASH} kind="address" />)).toThrow('not an address');
  });

  // A4 review 3: the piece copies through the one copy path (hooks/useCopy). A copy that cannot
  // work (no clipboard, as over plain HTTP on a LAN address; or a refusal) shows no tick and
  // rejects nothing; a copy that worked shows its tick, then the id again; no timer outlives it.
  describe('through the one copy path', () => {
    const rejections: unknown[] = [];
    const onRejection = (reason: unknown) => rejections.push(reason);
    beforeEach(() => {
      rejections.length = 0;
      process.on('unhandledRejection', onRejection);
    });
    afterEach(() => {
      process.off('unhandledRejection', onRejection);
      Object.assign(navigator, { clipboard: undefined });
      vi.useRealTimers();
    });
    // a click, then every pending promise and a macrotask for an unhandled rejection to surface
    const clickAndSettle = async () => {
      await act(async () => {
        fireEvent.click(head());
        await new Promise((resolve) => setTimeout(resolve, 0));
      });
    };

    // each clipboard made in its test (the file's afterEach restores every mock)
    for (const [label, clipboard] of [
      ['is absent', () => undefined],
      ['refuses', () => ({ writeText: vi.fn().mockRejectedValue(new DOMException('denied')) })],
    ] as const)
      it(`shows no tick and throws nothing where the clipboard ${label}`, async () => {
        stubLine(600);
        Object.assign(navigator, { clipboard: clipboard() });
        render(<HeadId value={HASH} kind="hash" copyable />);
        await clickAndSettle();
        expect(screen.queryByText('✓')).toBeNull();
        expect(lines()[0].parentElement).not.toHaveClass('invisible');
        expect(rejections).toEqual([]);
      });

    it('shows the tick, then the id again, and leaves no timer when it goes', async () => {
      stubLine(600);
      Object.assign(navigator, { clipboard: { writeText: vi.fn().mockResolvedValue(undefined) } });
      vi.useFakeTimers();
      const click = () =>
        act(async () => {
          fireEvent.click(head());
        });
      const { unmount } = render(<HeadId value={HASH} kind="hash" copyable />);
      await click();
      expect(screen.getByText('✓')).toBeInTheDocument();
      expect(vi.getTimerCount()).toBe(1);
      unmount();
      expect(vi.getTimerCount()).toBe(0);
      render(<HeadId value={HASH} kind="hash" copyable />);
      await click();
      expect(screen.getByText('✓')).toBeInTheDocument();
      act(() => vi.advanceTimersByTime(1500));
      expect(screen.queryByText('✓')).toBeNull();
      expect(lines()[0].parentElement).not.toHaveClass('invisible');
    });
  });
});
