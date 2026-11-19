import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { HeadId } from '@/components/ui/head-id';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import {
  boxPx,
  contentPx,
  gapPx,
  inFlowAt,
  insetsPx,
  shrinksAt,
  widthRuleAt,
} from '@/__tests__/utils/tailwind-box';

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

/**
 * The line's width and its type, 12 px by default, untracked. An id with its copy measures the line
 * of the row it shares with it, less the copy and the row's gap (lane c2-finish, rulings round 2):
 * the row and the id are `width` px, the copy (an element whose child is its button) `copyPx`, and
 * the row's gap 8 px, as the classes draw it.
 */
function stubLine(width: number, fontPx = 12, copyPx = 0) {
  stubChartWidth(width);
  const isCopy = (el: Element) => el.querySelector(':scope > button') !== null;
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
    this: HTMLElement
  ) {
    const w = isCopy(this) ? copyPx : width;
    return {
      width: w,
      height: 0,
      top: 0,
      left: 0,
      right: w,
      bottom: 0,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    } as DOMRect;
  });
  const real = window.getComputedStyle.bind(window);
  vi.spyOn(window, 'getComputedStyle').mockImplementation((el, pseudo) => {
    const style = real(el, pseudo);
    const row = (el as Element).querySelector(':scope > [data-head-id]') !== null;
    if (!(el as Element).hasAttribute('data-head-id') && !row) return style;
    return new Proxy(style, {
      get: (target, prop) =>
        prop === 'fontSize'
          ? `${fontPx}px`
          : prop === 'letterSpacing'
            ? 'normal'
            : prop === 'columnGap' && row
              ? '8px'
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

  // Re-pinned (lane c2-finish, item 4: copy has one form): the id was a click target whose tick
  // stood over it; now the id is text and the library's CopyButton after it copies it whole,
  // answering `copied` in words beside its icon while the id keeps its lines, never hidden.
  it('copies the whole id when it copies, keeping its lines', async () => {
    stubLine(326);
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    render(<HeadId value={HASH} kind="hash" copy="copy block hash" />);
    fireEvent.click(screen.getByRole('button', { name: 'copy block hash' }));
    expect(writeText).toHaveBeenCalledWith(HASH);
    expect(screen.getByTitle(HASH)).toBe(head());
    await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent(/^copied$/));
    expect(screen.queryByText('✓')).toBeNull();
    expect(lines()).toHaveLength(2);
    expect(head().querySelector('.invisible')).toBeNull();
  });

  // A4 round 2, task 4: a line narrower than one designed line (a hash's `0x` and a digit, 21.6 px
  // at 12 px; an address's one character, 7.2) is a layout state, not a broken invariant: it threw
  // and took the page down. Such a line cannot be designed, so the id is set as before it is
  // measured — one line, whole, the text and the title the whole id; never a throw.
  it('sets the id on one line where the line cannot hold one designed line', () => {
    for (const [value, kind, width] of [
      [HASH, 'hash', 21],
      [HASH, 'hash', 7],
      [ADDRESS, 'address', 7],
      [ADDRESS, 'address', 1],
    ] as const) {
      stubLine(width);
      const view = render(<HeadId value={value} kind={kind} />);
      expect(texts()).toEqual([value]);
      expect(head()).toHaveAttribute('title', value);
      view.unmount();
      vi.restoreAllMocks();
    }
    // from one designed line up it is designed: 22 px holds `0x` and a digit (3 × 7.2 = 21.6)
    stubLine(22);
    render(<HeadId value={HASH} kind="hash" />);
    expect(lines()).toHaveLength(64);
  });

  it('refuses a value that is not of its kind', () => {
    expect(() => render(<HeadId value="0x12g4" kind="hash" />)).toThrow('not a hex id');
    expect(() => render(<HeadId value={HASH} kind="address" />)).toThrow('not an address');
  });

  // A4 review 3: the piece copies through the one copy path (hooks/useCopy). A copy that cannot
  // work (no clipboard, as over plain HTTP on a LAN address; or a refusal) shows no tick and
  // rejects nothing; a copy that worked shows its tick, then the id again; no timer outlives it.
  // Re-pinned (lane c2-finish, item 4): the copy is the CopyButton after the id, so a copy that
  // cannot work says `not copied` and rejects nothing, one that worked says `copied` and then
  // nothing, and the id is never hidden; no timer outlives it.
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
        fireEvent.click(screen.getByRole('button', { name: 'copy block hash' }));
        await new Promise((resolve) => setTimeout(resolve, 0));
      });
    };

    // each clipboard made in its test (the file's afterEach restores every mock)
    for (const [label, clipboard] of [
      ['is absent', () => undefined],
      ['refuses', () => ({ writeText: vi.fn().mockRejectedValue(new DOMException('denied')) })],
    ] as const)
      it(`says not copied and throws nothing where the clipboard ${label}`, async () => {
        stubLine(600);
        Object.assign(navigator, { clipboard: clipboard() });
        render(<HeadId value={HASH} kind="hash" copy="copy block hash" />);
        await clickAndSettle();
        expect(screen.getByRole('status')).toHaveTextContent(/^not copied$/);
        expect(screen.queryByText('✓')).toBeNull();
        expect(head().querySelector('.invisible')).toBeNull();
        expect(rejections).toEqual([]);
      });

    it('says copied, then nothing, and leaves no timer when it goes', async () => {
      stubLine(600);
      Object.assign(navigator, { clipboard: { writeText: vi.fn().mockResolvedValue(undefined) } });
      vi.useFakeTimers();
      const click = () =>
        act(async () => {
          fireEvent.click(screen.getByRole('button', { name: 'copy block hash' }));
        });
      const { unmount } = render(<HeadId value={HASH} kind="hash" copy="copy block hash" />);
      await click();
      expect(screen.getByRole('status')).toHaveTextContent(/^copied$/);
      expect(vi.getTimerCount()).toBe(1);
      unmount();
      expect(vi.getTimerCount()).toBe(0);
      render(<HeadId value={HASH} kind="hash" copy="copy block hash" />);
      await click();
      expect(screen.getByRole('status')).toHaveTextContent(/^copied$/);
      act(() => vi.advanceTimersByTime(1500));
      expect(screen.getByRole('status')).toBeEmptyDOMElement();
      expect(head().textContent).toBe(HASH);
    });
  });

  // Lane c2-finish, item 4: copy has one form. A head's id is a value in the line's ink, and one
  // icon after it copies it: the id is text (no click of its own, no hover glow), the library's
  // CopyButton after it, 8 px apart, at the id's first line; the id measures the line the button
  // leaves it (its box 24 px from 640, 32 below, and the 6 px before its answer).
  describe('with its copy button after it', () => {
    it('is the id as text, the copy button after it, named for what it copies', () => {
      render(<HeadId value={HASH} kind="hash" copy="copy block hash" />);
      const button = screen.getByRole('button', { name: 'copy block hash' });
      expect(
        head().compareDocumentPosition(button) & Node.DOCUMENT_POSITION_FOLLOWING
      ).toBeTruthy();
      expect(head()).not.toContainElement(button);
      expect(head().className).not.toMatch(/cursor-pointer|hover-lit/);
      expect(head().textContent).toBe(HASH);
      const row = head().parentElement!;
      expect(row).toContainElement(button);
      expect(row).toHaveClass('flex', 'items-start', 'gap-2');
    });

    // Re-pinned (lane c2-finish, rulings round 1): the copy's answer is out of the line's flow,
    // so the id's line is the line less the gap and the box alone, 32 px from 640 and 40 below
    // (it was 38 and 46: the 6 px gap before the answer, and the answer's words beside the box
    // while they stood, 43.2 or 72 px more)
    it('leaves the id the line less 40 px below 640 and 32 from it, whatever the answer says', () => {
      render(<HeadId value={HASH} kind="hash" copy="copy block hash" />);
      const row = head().parentElement!;
      const button = screen.getByRole('button', { name: 'copy block hash' });
      const copyRoot = button.parentElement!;
      const taken = (w: number) => gapPx(row, w) + contentPx(copyRoot, w);
      expect([1280, 640, 390, 360].map(taken)).toEqual([32, 32, 40, 40]);
      expect(boxPx(button, 390)).toBe(32);
      expect(inFlowAt(within(copyRoot).getByRole('status'), 390)).toBe(false);
      expect(shrinksAt(copyRoot, 390)).toBe(false);
    });

    // An id in two or three lines stands beside its box: an answer hung under the box alone would
    // lie over the id's later lines. The copy stands the row's full height, its icon at the
    // first line, so its answer hangs under the whole id, over none of it.
    // Lane c2-finish, rulings round 2: the copy stood at the far end of the row (at 1280, x = 1226,
    // 352 to 727 px after the id's last glyph), because the id grew to fill the row. The id takes
    // its content's width (it may shrink and wrap; it does not grow), so the copy stands the row's
    // 8 px gap after it wherever the row has room, and after its lines' box where it wraps.
    it('stands the copy 8 px after the id: the id takes its content’s width, never the row’s', () => {
      render(<HeadId value={HASH} kind="hash" copy="copy block hash" />);
      const row = head().parentElement!;
      const copyRoot = screen.getByRole('button', { name: 'copy block hash' }).parentElement!;
      for (const w of [1280, 640, 390, 360]) {
        expect(widthRuleAt(head(), w), `the id at ${w}`).toBe('auto');
        expect(shrinksAt(head(), w)).toBe(true);
        expect(widthRuleAt(copyRoot, w)).toBe('auto');
        expect(insetsPx(copyRoot, w).margin).toBe(0);
        expect(gapPx(row, w)).toBe(8);
      }
      expect(row.className).not.toMatch(/justify-(between|end|around|evenly)/);
      expect(copyRoot.className).not.toMatch(/(^|\s)ml-auto/);
    });

    it('hangs the answer under the whole id, the icon at its first line', () => {
      render(<HeadId value={HASH} kind="hash" copy="copy block hash" />);
      const button = screen.getByRole('button', { name: 'copy block hash' });
      const copyRoot = button.parentElement!;
      expect(copyRoot).toHaveClass('self-stretch', 'items-start');
      expect(copyRoot).not.toHaveClass('items-center');
      expect(within(copyRoot).getByRole('status')).toHaveClass('top-full');
    });

    // Re-pinned (lane c2-finish, rulings round 2): the id no longer fills its row (it takes its
    // content's width, so its copy stands 8 px after it), so it measures the row's line less the
    // copy's box and the gap: the stub gives the row 608 and 358 px and the copy 24 and 32 (it gave
    // the id the line left over, 576 and 318)
    it('keeps a hash at 13 px in the lines it had: one at 640 (576 px of 608), two at 390', () => {
      // 66 × 7.8 = 514.8 px: 608 − 24 − 8 = 576 holds it; at 390, 358 − 32 − 8 = 318 holds a half
      // (34 × 7.8 = 265.2)
      stubLine(608, 13, 24);
      const one = render(<HeadId value={HASH} kind="hash" copy="copy block hash" />);
      expect(lines()).toHaveLength(1);
      one.unmount();
      vi.restoreAllMocks();
      stubLine(358, 13, 32);
      render(<HeadId value={HASH} kind="hash" copy="copy block hash" />);
      expect(texts()).toEqual([HASH.slice(0, 34), HASH.slice(34)]);
    });

    it('sets a DID as an id of its own kind, in equal lines where the line cannot hold it', () => {
      const DID = 'did:ckb:ceirceirceirceirceirceirceirceir';
      stubLine(1000, 14);
      const one = render(<HeadId value={DID} kind="did" copy="copy DID" />);
      expect(texts()).toEqual([DID]);
      one.unmount();
      vi.restoreAllMocks();
      // 40 × 8.4 = 336 px; 312 holds 37 characters: two lines of 20
      stubLine(312, 14);
      render(<HeadId value={DID} kind="did" copy="copy DID" />);
      expect(texts()).toEqual([DID.slice(0, 20), DID.slice(20)]);
      expect(() => render(<HeadId value="did:web:x" kind="did" />)).toThrow('not a did:ckb');
    });
  });
});
