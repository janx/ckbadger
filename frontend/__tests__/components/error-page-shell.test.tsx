import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import defaultTheme from 'tailwindcss/defaultTheme';
import { MemoryRouter } from 'react-router-dom';
import { render as bareRender } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { http, HttpResponse } from 'msw';
import { QueryClientProvider } from '@tanstack/react-query';
import {
  createTestQueryClient,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@/__tests__/utils/test-utils';
import { server } from '@/__tests__/msw/server';
import { parsed, SEAL_INKS } from '@/__tests__/utils/seals';
import { drawSeal, type SealId } from '@/lib/seal/stones';
import { CrashPage } from '@/components/crash-page';
import { ErrorBoundary } from '@/components/error-boundary';
import { ERROR_PAGE_LAYOUT, ErrorPageShell } from '@/components/error-page-shell';
import { inkOf } from '@/__tests__/utils/ink-role';
import { displayAt, textRunPx } from '@/__tests__/utils/tailwind-box';
import { WIDEST_BLOCK_NUMBER } from '@/lib/list-bounds';

vi.mock('@/src/navigation', async () => await vi.importActual('@/src/navigation'));

const API = '/api/:network/v1';
/**
 * The tip's block (`/statistics/network` says #1,000,000) as MSW's `/blocks/:id` serves it: this
 * hash (32 lowercase bytes) and 5 transactions.
 */
const TIP_HASH = '0xabc123def456789012345678901234567890123456789012345678901234abcd';
const VERSE = [
  'no page at this path.',
  'the chain was not asked; nothing on it is lost.',
  "honey badger don't care.",
] as const;
const VERSE_NAME = VERSE.join(' ');

describe('ErrorPageShell', () => {
  it('is the verse in the voice: the h1 as three comment lines, the eyebrow and the path, no card', () => {
    const path = '/tx/' + 'a'.repeat(128);
    render(
      <ErrorPageShell status="Not found · 404" path={path} verse={VERSE} contextSafe>
        <span>the ways on</span>
      </ErrorPageShell>
    );
    const verse = screen.getByRole('heading', { level: 1, name: VERSE_NAME });
    expect(verse).toHaveClass('comment', 'comment-lg');
    expect(verse).not.toHaveAttribute('style');
    expect(Array.from(verse.querySelectorAll('[data-comment-line]'), (l) => l.textContent)).toEqual(
      [...VERSE]
    );
    // the sign's tubes are gone
    expect(document.querySelector('[data-tube], [data-sign], .sign')).toBeNull();
    const eyebrow = screen.getByText('Not found · 404');
    expect(eyebrow).toHaveClass('eyebrow');
    // the eyebrow line is a prompt: the cinnabar square before it
    expect(eyebrow.previousElementSibling).toHaveClass('section-mark');
    expect(screen.getByText(path)).toHaveClass('break-all');
    expect(screen.getByText('the ways on').parentElement).toHaveClass('error-facts');
    expect(screen.getByRole('main').parentElement).toHaveClass('overflow-x-hidden');
    expect(screen.queryByRole('contentinfo')).toBeNull();
    // provider-safe: no query client, so no tip to read and no seal; no canvas, no night
    expect(screen.queryByTestId('error-seal')).toBeNull();
    expect(document.querySelector('[data-seal], [data-hash-seal]')).toBeNull();
    expect(document.querySelector('canvas, .error-night')).toBeNull();
    // a whole screen ends on the empty prompt, as the frame's pages do
    expect(screen.getAllByTestId('prompt-end')).toHaveLength(1);
  });

  it('the eyebrow says what it means while the pointer is on it', () => {
    render(
      <ErrorPageShell status="Not found · 404" path="/nowhere" verse={VERSE} contextSafe>
        <span>facts</span>
      </ErrorPageShell>
    );
    const eyebrow = screen.getByText('Not found · 404');
    fireEvent.pointerEnter(eyebrow);
    expect(eyebrow).toHaveTextContent('You are (not) here');
    fireEvent.pointerLeave(eyebrow);
    expect(eyebrow).toHaveTextContent('Not found · 404');
  });

  // The seals by kind (C5): the newest block's square stone, its transactions counted in rods, in
  // place of the old brush seal of its hash.
  // Re-pinned on purpose (readability R6): the stone stands in its own place clear of the text,
  // 256 px (it was 469, behind the verse), main no longer isolates a stack for it, and the caption
  // under it says no place ("behind: " went); renamed: "presses the newest block's seal behind
  // the verse".
  it('with providers it presses the newest block’s seal in its own place: the tip block’s stone, once', async () => {
    render(
      <MemoryRouter initialEntries={['/mainnet/nowhere']}>
        <ErrorPageShell status="Not found · 404" path="/nowhere" verse={VERSE}>
          <span>facts</span>
        </ErrorPageShell>
      </MemoryRouter>
    );
    const seal = await screen.findByTestId('error-seal');
    // the drawing is a lazy chunk: the seal library is not in the entry
    await waitFor(() => expect(seal.querySelector('[data-seal] svg')).not.toBeNull());
    // decoration beside the text: hidden from the reader, who has the caption
    expect(seal).toHaveAttribute('aria-hidden', 'true');
    expect(seal).toHaveClass('error-seal');
    // one seal on the page, the stone and not the old hash seal
    expect(document.querySelectorAll('[data-seal]')).toHaveLength(1);
    expect(document.querySelector('[data-hash-seal]')).toBeNull();
    const drawn = seal.querySelector<HTMLElement>('[data-seal]')!;
    expect(seal.firstElementChild).toBe(drawn);
    expect(drawn).toHaveAttribute('data-seal', 'block');
    expect(drawn).toHaveAttribute('data-seal-form', 'page');
    expect(drawn).toHaveAttribute('data-seal-state', 'pressed');
    // the tip's block as /blocks/{n} serves it: its hash and its 5 transactions, drawn 256 px
    // square (the room beside the text column at 1280; it was 469, the area of the old 306 × 720
    // brush seal), pressed −3° (the not-found board's hand)
    const id: SealId = { kind: 'block', hash: TIP_HASH, txs: 5 };
    expect(drawn.innerHTML).toBe(
      parsed(
        drawSeal(id, { form: 'page', height: 256, state: 'pressed', ...SEAL_INKS, tilt: -3 }).svg
      )
    );
    const svg = drawn.querySelector('svg')!;
    expect(svg).toHaveAttribute('aria-label', 'Block seal 0xabc123de, 5 transactions');
    expect(svg).toHaveAttribute('width', '256');
    expect(svg).toHaveAttribute('height', '256');
    // in the cinnabar mark, pressed a little askew; faint is the CSS's (--m-seal-press)
    expect(drawn.innerHTML).toContain('var(--c-mark)');
    expect(drawn.innerHTML).not.toContain('var(--c-rouge)');
    expect(svg.getAttribute('style')).toBe('transform: rotate(-3deg)');
    // the seal sits in main, in the page's flow; nothing is stacked under the text
    expect(screen.getByRole('main')).toContainElement(seal);
    expect(screen.getByRole('main')).not.toHaveClass('isolate');
    // the caption under the stone names the block, as a link, and its hash
    const caption = document.querySelector('[data-seal-caption]') as HTMLElement;
    expect(caption.tagName).toBe('FIGCAPTION');
    expect(caption.parentElement).toBe(seal.parentElement);
    expect(caption).toHaveTextContent(
      /^the seal of the newest block, #1,000,000 · 0xabc123de…34abcd$/
    );
    // R4 (UI review 2026-11-14, U-08): a caption is information, in the dim ink, never the ghost
    expect(caption).toHaveClass('text-text-dim', 'text-meta');
    expect(caption).not.toHaveClass('text-text-ghost');
    expect(within(caption).getByRole('link', { name: '#1,000,000' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/1000000'
    );
    // readability ruling R3: the caption is a note, quiet; the block's link and its hash in it are
    // a link and a value, the reading ink
    expect(inkOf(within(caption).getByRole('link', { name: '#1,000,000' }))).toBe('text-text');
    expect(inkOf(within(caption).getByText('0xabc123de…34abcd'))).toBe('text-text');
    // no WebGL, no CSS night, no 256 squares
    expect(document.querySelector('canvas, .error-night, .error-night-bits')).toBeNull();
  });

  it('presses no seal when the tip’s block cannot be read: the verse stands alone, no hash made up', async () => {
    server.use(
      http.get(`${API}/blocks/:id`, () =>
        HttpResponse.json({ error: 'not_found', message: 'no block' }, { status: 404 })
      )
    );
    const client = createTestQueryClient();
    bareRender(
      <QueryClientProvider client={client}>
        <MemoryRouter initialEntries={['/mainnet/nowhere']}>
          <ErrorPageShell status="Not found · 404" path="/nowhere" verse={VERSE}>
            <span>facts</span>
          </ErrorPageShell>
        </MemoryRouter>
      </QueryClientProvider>
    );
    // the vitals named the tip, and reading its block failed (a 404 is the API's definite answer)
    await waitFor(() => expect(client.getQueryState(['block', 1000000])?.status).toBe('error'));
    expect(screen.queryByTestId('error-seal')).toBeNull();
    expect(document.querySelector('[data-seal], [data-hash-seal], [data-seal-caption]')).toBeNull();
    expect(screen.getByRole('heading', { level: 1, name: VERSE_NAME })).toBeInTheDocument();
  });

  it('presses no seal before the vitals name a tip', () => {
    server.use(http.get(`${API}/statistics/network`, () => new Promise<never>(() => {})));
    render(
      <MemoryRouter initialEntries={['/mainnet/nowhere']}>
        <ErrorPageShell status="Not found · 404" path="/nowhere" verse={VERSE}>
          <span>facts</span>
        </ErrorPageShell>
      </MemoryRouter>
    );
    expect(screen.queryByTestId('error-seal')).toBeNull();
    expect(document.querySelector('[data-seal], [data-seal-caption]')).toBeNull();
  });

  it('presses no seal while the tip’s block is being read', async () => {
    server.use(http.get(`${API}/blocks/:id`, () => new Promise<never>(() => {})));
    const client = createTestQueryClient();
    bareRender(
      <QueryClientProvider client={client}>
        <MemoryRouter initialEntries={['/mainnet/nowhere']}>
          <ErrorPageShell status="Not found · 404" path="/nowhere" verse={VERSE}>
            <span>facts</span>
          </ErrorPageShell>
        </MemoryRouter>
      </QueryClientProvider>
    );
    // the vitals named the tip, and its block is being asked for
    await waitFor(() =>
      expect(client.getQueryState(['block', 1000000])?.fetchStatus).toBe('fetching')
    );
    expect(screen.queryByTestId('error-seal')).toBeNull();
    expect(document.querySelector('[data-seal], [data-seal-caption]')).toBeNull();
    expect(screen.getByRole('heading', { level: 1, name: VERSE_NAME })).toBeInTheDocument();
  });

  // Re-pinned on purpose (readability R6): the stone took about the area of the old brush seal
  // behind the verse, 0.65 of main's height (`height: min(65%, 65svh, calc(100vw - 32px))`);
  // standing in its own place it is shown at the size it is drawn, 256 px, the room beside the
  // text column at 1280 (the R6 tests below), and the CSS sets no size (renamed: "sizes the stone
  // from the first screen").
  it('shows the stone at the size it is drawn, faint, with no size of its own in the CSS', () => {
    const css = readFileSync(resolve(__dirname, '../../app/globals.css'), 'utf-8');
    const rule = (selector: string) => {
      const at = css.indexOf(`\n${selector} {`);
      expect(at).toBeGreaterThan(-1);
      return css.slice(at, css.indexOf('}', at));
    };
    expect(rule('.error-seal')).toContain('opacity: var(--m-seal-press);');
    expect(rule('.error-seal > [data-seal],\n.error-seal svg')).toContain('display: block;');
    expect(css.slice(css.indexOf('\n.error-seal {'))).not.toMatch(
      /^\.error-seal[^{]*\{[^}]*(height|width):/m
    );
    // the old hash seal's rule is gone with it
    expect(css).not.toContain('[data-hash-seal]');
  });

  it('the crash page: the thought of the failed transition, the error in red, one footer, and no claim about the chain', () => {
    // drawn in place of the frame (the frame route's boundary): its own header and footer
    render(
      <MemoryRouter initialEntries={['/mainnet/fiber/channels/0x01']}>
        <CrashPage message="TypeError: missing channel output" standalone />
      </MemoryRouter>
    );
    // its own words, in the voice; the 404's meme is not quoted here
    const verse = screen.getByRole('heading', {
      level: 1,
      name: 'the chain keeps what succeeded. this did not.',
    });
    expect(verse).toHaveClass('comment', 'comment-lg');
    expect(screen.queryByText(/honey badger/)).toBeNull();
    expect(screen.getByText('Error · not a 404')).toHaveClass('eyebrow');
    expect(screen.getByRole('alert')).toHaveTextContent('✗ TypeError: missing channel output');
    expect(screen.getAllByText('/mainnet/fiber/channels/0x01')).toHaveLength(2);
    // the facts wrap between words; only the route, one long token, breaks anywhere
    const route = screen.getAllByText('/mainnet/fiber/channels/0x01')[1];
    expect(route).toHaveClass('break-all');
    expect(route.parentElement).not.toHaveClass('break-all');
    expect(route.parentElement).toHaveClass('[overflow-wrap:anywhere]');
    expect(
      screen.getByText(
        'the chain was not checked in this view — this says nothing about what it holds'
      )
    ).toBeInTheDocument();
    expect(
      screen.queryByText(/nothing is wrong with the chain|reloading will not mend/i)
    ).toBeNull();
    expect(screen.getByRole('button', { name: '↻ try again' })).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: '→ copy details for a bug report' })
    ).toBeInTheDocument();
    expect(screen.getByRole('link', { name: '← Fiber channels' })).toHaveAttribute(
      'href',
      '/mainnet/fiber/channels'
    );
    expect(screen.getAllByRole('contentinfo')).toHaveLength(1);
    expect(document.querySelector('canvas, .error-night')).toBeNull();
  });

  // Visual finish V6: in the frame (the default) the frame draws the header and the footer; the
  // shell draws neither and fills what the header leaves (flex-1, not a whole screen).
  it('in the frame: no header and no footer of its own, and it fills what the header leaves', () => {
    render(
      <MemoryRouter initialEntries={['/mainnet/fiber/channels/0x01']}>
        <CrashPage message="TypeError: missing channel output" />
      </MemoryRouter>
    );
    expect(screen.getByRole('alert')).toHaveTextContent('✗ TypeError: missing channel output');
    expect(screen.queryByRole('banner')).toBeNull();
    expect(screen.queryByRole('contentinfo')).toBeNull();
    const shell = screen.getByRole('main').parentElement!;
    expect(shell).toHaveClass('flex-1');
    expect(shell).not.toHaveClass('min-h-screen');
  });

  it('drawn in place of the frame: its own header and footer, once each, a whole screen', () => {
    render(
      <MemoryRouter initialEntries={['/mainnet/fiber/channels/0x01']}>
        <CrashPage message="TypeError: missing channel output" standalone />
      </MemoryRouter>
    );
    expect(screen.getAllByRole('banner')).toHaveLength(1);
    expect(screen.getAllByRole('contentinfo')).toHaveLength(1);
    const shell = screen.getByRole('main').parentElement!;
    expect(shell).toHaveClass('min-h-screen');
    expect(shell).not.toHaveClass('flex-1');
  });

  it('renders a provider-safe fallback when the app itself throws', () => {
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {});
    function ThrowingChild(): never {
      throw new Error('block 12: invalid DAO field');
    }
    try {
      bareRender(
        <ErrorBoundary>
          <ThrowingChild />
        </ErrorBoundary>
      );
      expect(screen.getByRole('alert')).toHaveTextContent('block 12: invalid DAO field');
      expect(screen.getByRole('link', { name: '← back to the explorer' })).toHaveAttribute(
        'href',
        '/mainnet'
      );
      expect(screen.queryByRole('navigation', { name: 'Main navigation' })).toBeNull();
      expect(document.querySelector('canvas, .error-night')).toBeNull();
      // above every provider: no tip to read, so the verse stands without a seal
      expect(document.querySelector('[data-seal], [data-hash-seal]')).toBeNull();
      expect(
        screen.getByRole('heading', {
          level: 1,
          name: 'the chain keeps what succeeded. this did not.',
        })
      ).toBeInTheDocument();
      expect(screen.getByRole('link', { name: 'CKBadger home' })).toHaveAttribute('href', '/');
    } finally {
      consoleError.mockRestore();
    }
  });

  it('copies exact error context without inventing a chain tip', async () => {
    const oldClipboard = Object.getOwnPropertyDescriptor(navigator, 'clipboard');
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    });
    try {
      render(
        <MemoryRouter initialEntries={['/mainnet/tx/0xabc']}>
          <CrashPage message="block 12: invalid DAO field" />
        </MemoryRouter>
      );
      fireEvent.click(screen.getByRole('button', { name: '→ copy details for a bug report' }));
      await waitFor(() =>
        expect(within(screen.getByRole('main')).getByRole('status')).toHaveTextContent('copied')
      );
      expect(writeText).toHaveBeenCalledWith(
        expect.stringContaining('Error: block 12: invalid DAO field')
      );
      expect(writeText).toHaveBeenCalledWith(expect.stringContaining('Page: /mainnet/tx/0xabc'));
      expect(writeText).toHaveBeenCalledWith(expect.stringContaining('Network: mainnet'));
      expect(writeText).toHaveBeenCalledWith(
        expect.stringContaining('Chain tip: not checked in this error view')
      );
    } finally {
      if (oldClipboard) Object.defineProperty(navigator, 'clipboard', oldClipboard);
      else Reflect.deleteProperty(navigator, 'clipboard');
    }
  });
});

// Readability ruling R6 (the review: by night the stone lay under the verse and the facts, its red
// strokes through their glyphs): the newest block's seal stands clear of the text column at every
// width. jsdom does not lay out, so the boxes are the page's own layout constants
// (ERROR_PAGE_LAYOUT), tied below to the classes and the drawing that make them, and the
// arithmetic is done here at the five widths the lead measures: 1280, 1024, 640, 390, 360.
describe('the pressed seal stands clear of the text column (readability R6)', () => {
  const { gutters, besideFrom, textColumnPx, sealGapPx, sealPx, sealTilt } = ERROR_PAGE_LAYOUT;
  const WIDTHS = [1280, 1024, 640, 390, 360] as const;
  interface Box {
    left: number;
    right: number;
    top: number;
    bottom: number;
  }
  /** The side gutter at a window width: the column's padding at its breakpoint. */
  const gutterAt = (width: number) => [...gutters].reverse().find((g) => width >= g.from)!.px;
  /** How far a square of `side` turned by `tilt` degrees draws past its box on every side. */
  const turnedPast = (side: number, tilt: number) => {
    const turn = (Math.abs(tilt) * Math.PI) / 180;
    return (side * (Math.cos(turn) + Math.sin(turn)) - side) / 2;
  };
  const meet = (a: Box, b: Box) =>
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom;
  /**
   * The text column's box and the stone's drawn box at a window width, px from the column's top
   * left corner on the page, for a column `textHeight` tall (whatever its words make it).
   */
  function boxes(width: number, textHeight: number) {
    const gutter = gutterAt(width);
    const inner = width - 2 * gutter;
    const beside = width >= besideFrom;
    const textWidth = beside ? Math.min(textColumnPx, inner - sealGapPx - sealPx) : inner;
    const text: Box = { left: gutter, right: gutter + textWidth, top: 0, bottom: textHeight };
    const x = beside ? gutter + textWidth + sealGapPx : gutter;
    const y = beside ? 0 : textHeight + sealGapPx;
    const past = turnedPast(sealPx, sealTilt);
    const seal: Box = {
      left: x - past,
      right: x + sealPx + past,
      top: y - past,
      bottom: y + sealPx + past,
    };
    return { gutter, inner, beside, text, seal };
  }
  const tenth = (px: number) => Math.round(px * 100) / 100;

  it('turned −3°, the 256 px stone draws 269.05 px across, 6.52 past its box on every side', () => {
    expect(tenth(sealPx + 2 * turnedPast(sealPx, sealTilt))).toBe(269.05);
    expect(tenth(turnedPast(sealPx, sealTilt))).toBe(6.52);
    // at 1280 the column, the gap and the stone are exactly what the gutters leave
    expect(textColumnPx + sealGapPx + sealPx).toBe(1280 - 2 * gutterAt(1280));
  });

  it.each(WIDTHS)(
    'at %i px the stone, as drawn, never meets the text column and stays on the page',
    (width) => {
      for (const textHeight of [80, 240, 480, 960]) {
        const { text, seal } = boxes(width, textHeight);
        expect(
          meet(text, seal),
          `${width} px: text column ${JSON.stringify(text)}, stone ${JSON.stringify(seal)}`
        ).toBe(false);
        expect(seal.left).toBeGreaterThanOrEqual(0);
        expect(seal.right).toBeLessThanOrEqual(width);
      }
    }
  );

  // The arithmetic per width, as the hand-back states it: the gutter, the room between the
  // gutters, where the stone stands, the text column across, the stone's drawn box across, and the
  // clear space between the column and the drawn stone (across beside it, down below it).
  it('states the boxes at each width', () => {
    const rows = WIDTHS.map((width) => {
      const { gutter, inner, beside, text, seal } = boxes(width, 240);
      return {
        width,
        gutter,
        inner,
        stone: beside ? 'beside' : 'below',
        text: [text.left, text.right],
        drawn: [tenth(seal.left), tenth(seal.right)],
        clear: tenth(beside ? seal.left - text.right : seal.top - text.bottom),
      };
    });
    expect(rows).toEqual([
      {
        width: 1280,
        gutter: 96,
        inner: 1088,
        stone: 'beside',
        text: [96, 864],
        drawn: [921.48, 1190.52],
        clear: 57.48,
      },
      {
        width: 1024,
        gutter: 96,
        inner: 832,
        stone: 'below',
        text: [96, 928],
        drawn: [89.48, 358.52],
        clear: 57.48,
      },
      {
        width: 640,
        gutter: 32,
        inner: 576,
        stone: 'below',
        text: [32, 608],
        drawn: [25.48, 294.52],
        clear: 57.48,
      },
      {
        width: 390,
        gutter: 16,
        inner: 358,
        stone: 'below',
        text: [16, 374],
        drawn: [9.48, 278.52],
        clear: 57.48,
      },
      {
        width: 360,
        gutter: 16,
        inner: 328,
        stone: 'below',
        text: [16, 344],
        drawn: [9.48, 278.52],
        clear: 57.48,
      },
    ]);
  });

  // The numbers above are the page's: its breakpoints are Tailwind's, its padding and grid classes
  // are these constants, the stone is drawn at sealPx and turned sealTilt, and it stands in the
  // page's flow, never pressed over the box that holds the text.
  it('lays the page out by those constants: the text column, then the stone in its own place', async () => {
    const css = readFileSync(resolve(__dirname, '../../app/globals.css'), 'utf-8');
    const at = css.indexOf('\n.error-seal {');
    expect(at, 'globals.css has no .error-seal rule').toBeGreaterThan(-1);
    const sealRule = css.slice(at, css.indexOf('}', at));
    // pressed over main's whole box (absolute, inset 0, z −1) it lay under every line of text
    expect(sealRule, 'the stone is pressed over main, which holds the text column').not.toMatch(
      /position:\s*absolute|inset:|z-index:/
    );
    expect(sealRule).toContain('opacity: var(--m-seal-press);');
    expect(Number.parseInt(defaultTheme.screens.sm, 10)).toBe(gutters[1].from);
    expect(Number.parseInt(defaultTheme.screens.lg, 10)).toBe(gutters[2].from);
    expect(Number.parseInt(defaultTheme.screens.xl, 10)).toBe(besideFrom);
    render(
      <MemoryRouter initialEntries={['/mainnet/nowhere']}>
        <ErrorPageShell status="Not found · 404" path="/nowhere" verse={VERSE}>
          <span>facts</span>
        </ErrorPageShell>
      </MemoryRouter>
    );
    const stone = await screen.findByTestId('error-seal');
    await waitFor(() => expect(stone.querySelector('[data-seal] svg')).not.toBeNull());
    const layout = screen.getByRole('main').querySelector<HTMLElement>('[data-error-layout]');
    expect(layout, 'the page has no layout of the text column and the stone').not.toBeNull();
    expect(layout).toHaveClass(
      ...gutters.map(({ from, px }) =>
        from === 0 ? `px-${px / 4}` : `${from === 640 ? 'sm' : 'lg'}:px-${px / 4}`
      ),
      'grid',
      'grid-cols-[minmax(0,1fr)]',
      `gap-y-${sealGapPx / 4}`,
      `xl:grid-cols-[minmax(0,${textColumnPx}px)_${sealPx}px]`,
      `xl:gap-x-${sealGapPx / 4}`
    );
    // two places: the text column (the eyebrow, the verse, the facts), then the stone's
    const [text, figure] = Array.from(layout!.children) as HTMLElement[];
    expect(layout!.children).toHaveLength(2);
    expect(text).toHaveAttribute('data-error-text');
    expect(text).toContainElement(screen.getByRole('heading', { level: 1, name: VERSE_NAME }));
    expect(text).toContainElement(screen.getByText('facts'));
    expect(text).not.toContainElement(stone);
    expect(figure.tagName).toBe('FIGURE');
    expect(figure.firstElementChild).toBe(stone);
    // the stone's box is sealPx square before its drawing arrives, and drawn sealPx, turned
    expect(stone.style.width).toBe(`${sealPx}px`);
    expect(stone.style.height).toBe(`${sealPx}px`);
    const svg = stone.querySelector('svg')!;
    expect(svg).toHaveAttribute('width', String(sealPx));
    expect(svg).toHaveAttribute('height', String(sealPx));
    expect(svg.getAttribute('style')).toBe(`transform: rotate(${sealTilt}deg)`);
  });
});

// The coordinator's rule for the caption under the 404 seal: a line never starts with a separator.
// When the caption wraps, its " · " is never the first thing drawn on a line. The site has one
// owner for a separator between parts, Dotted (components/ui/phone-row.tsx: a separator belongs to
// the item after it, a line breaks before it, never after it, and it is not drawn where its item
// starts a line), so every "·" of the caption is a Dotted separator. jsdom lays nothing out: the
// caption's units are read from the rendered DOM (its words, and every box a line does not break
// inside: a nowrap run, an inline list), each box's display read at the window through
// tailwind-box (`displayAt`, as line-runs reads it), measured at the caption's own step, and set
// greedily on lines as wide as the caption's box at each window (ERROR_PAGE_LAYOUT: the stone's
// 256 px column from 1280, the room between the gutters below). A block-level box (a list drawn
// `flex`, not `inline-flex`) stands on lines of its own: it closes the line before it and the
// next unit starts a new one (review rules-tests SHOULD-3: the units were all read as inline, so a
// caption whose list had lost `inline` passed while Chrome drew it on a line of its own). What the
// reader cannot lay out it refuses: a block wider than its line, a box of any other display. The
// block number is counted at its widest (`WIDEST_BLOCK_NUMBER`), whatever the tip is.
describe('the caption under the 404 seal never opens a line with a separator', () => {
  const { gutters, besideFrom, sealPx } = ERROR_PAGE_LAYOUT;
  const gutterAt = (width: number) => [...gutters].reverse().find((g) => width >= g.from)!.px;
  /** The caption's box at a window: the stone's column from xl, else the column's whole width. */
  const captionPx = (viewport: number) =>
    viewport >= besideFrom ? sealPx : viewport - 2 * gutterAt(viewport);
  /** The windows checked: the coordinator's four, and every width the caption wraps at today. */
  const VIEWPORTS = [320, 360, 390, 640, 1024, 1280, 1440] as const;

  interface Unit {
    /** The text drawn, the block number at its widest. */
    text: string;
    /** A line may break before it (a space precedes it). */
    breakBefore: boolean;
    /** An inline Dotted list: one box on a line, its separators its own (hidden at a line start). */
    dotted: boolean;
    /** A block-level box: it stands on lines of its own. */
    block: boolean;
  }
  /** The block-level displays (line-runs' set): such a box opens and closes its own lines. */
  const BLOCK_LEVEL = new Set(['block', 'flex', 'grid', 'list-item', 'table']);
  /** The text an element draws, a block number written at its widest. */
  const drawn = (node: Node): string =>
    node instanceof HTMLAnchorElement && /\/blocks\/\d+$/.test(node.getAttribute('href') ?? '')
      ? WIDEST_BLOCK_NUMBER
      : node.nodeType === 3
        ? (node.textContent ?? '')
        : Array.from(node.childNodes).map(drawn).join('');
  /** The caption's units in order at a window: words, and the boxes a line never breaks inside. */
  function units(root: Element, viewport: number): Unit[] {
    const out: Unit[] = [];
    let space = false;
    const walk = (node: Node) => {
      if (node.nodeType === 3) {
        for (const piece of (node.textContent ?? '').split(/(\s+)/)) {
          if (piece === '') continue;
          if (/^\s+$/.test(piece)) space = true;
          else {
            out.push({ text: piece, breakBefore: space, dotted: false, block: false });
            space = false;
          }
        }
        return;
      }
      const el = node as HTMLElement;
      const display = displayAt(el, viewport);
      const block = BLOCK_LEVEL.has(display);
      const dotted = el.hasAttribute('data-dotted');
      if (dotted && !block && display !== 'inline-flex')
        throw new Error(
          `${viewport}: a separated list drawn ${display}, which the reader does not lay out`
        );
      if (dotted || el.classList.contains('whitespace-nowrap') || drawn(el) !== el.textContent) {
        const text = drawn(el).replace(/\s+/g, ' ');
        if (/^\s/.test(text)) space = true;
        out.push({
          text: text.trim(),
          breakBefore: space || block,
          dotted: dotted && !block,
          block,
        });
        space = /\s$/.test(text) || block;
        return;
      }
      if (block)
        throw new Error(
          `${viewport}: a ${display} box in the caption, which the reader does not lay out`
        );
      el.childNodes.forEach(walk);
    };
    root.childNodes.forEach(walk);
    return out;
  }
  /** The caption's lines at a window, greedily filled, each line's units. */
  function lines(caption: HTMLElement, viewport: number): Unit[][] {
    const width = captionPx(viewport);
    const px = (text: string) => textRunPx(text, caption, viewport);
    const out: Unit[][] = [[]];
    let x = 0;
    let closed = false;
    for (const unit of units(caption, viewport)) {
      const w = px(unit.text);
      if (w > width)
        throw new Error(`${viewport}: "${unit.text}" (${w} px) is wider than its ${width} px line`);
      const line = out[out.length - 1];
      const need = line.length === 0 ? w : (unit.breakBefore ? px(' ') : 0) + w;
      // a block-level box closes the line before it and the line it stands on
      if (line.length > 0 && (unit.block || closed || (x + need > width && unit.breakBefore))) {
        out.push([unit]);
        x = w;
      } else {
        line.push(unit);
        x += need;
      }
      closed = unit.block;
    }
    return out;
  }
  const text = (line: Unit[]) =>
    line.map((u, i) => (i > 0 && u.breakBefore ? ' ' : '') + u.text).join('');

  it('reads the caption’s list as one inline box, a line of its own when it is a block', async () => {
    const node = await caption();
    const list = node.querySelector('[data-dotted]')!;
    // the caption passes `inline`: the list is inline-flex at every window, never a block
    for (const viewport of VIEWPORTS) expect(displayAt(list, viewport)).toBe('inline-flex');
    expect(units(node, 640).map((u) => [u.text.slice(0, 11), u.dotted, u.block])).toEqual([
      ['the', false, false],
      ['seal', false, false],
      ['of', false, false],
      ['the', false, false],
      ['newest', false, false],
      ['block,', false, false],
      [WIDEST_BLOCK_NUMBER, true, false],
    ]);
  });

  async function caption(): Promise<HTMLElement> {
    render(
      <MemoryRouter initialEntries={['/mainnet/nowhere']}>
        <ErrorPageShell status="Not found · 404" path="/nowhere" verse={VERSE}>
          <span>facts</span>
        </ErrorPageShell>
      </MemoryRouter>
    );
    await screen.findByTestId('error-seal');
    return document.querySelector('[data-seal-caption]') as HTMLElement;
  }

  it.each(VIEWPORTS)('at %i px no line of the caption opens with a separator', async (viewport) => {
    const node = await caption();
    const opened = lines(node, viewport)
      .slice(1)
      .filter((line) => !line[0].dotted && line[0].text.startsWith('·'))
      .map((line) => text(line));
    expect(opened, `${viewport} px: a line opens with a separator`).toEqual([]);
  });

  it('draws every separator of the caption as Dotted’s, in the item after it', async () => {
    const node = await caption();
    const dots = Array.from(node.querySelectorAll('*')).filter((el) =>
      Array.from(el.childNodes).some((c) => c.nodeType === 3 && c.textContent!.includes('·'))
    );
    expect(dots.length).toBeGreaterThan(0);
    for (const dot of dots) {
      expect(dot).toHaveAttribute('data-dotted-separator');
      expect(dot.closest('[data-dotted-item]')).not.toBeNull();
    }
  });

  // Where the caption breaks, at the widest block number: the words (29 glyphs at 12 px, 208.8),
  // then "#99,999,999 · 0xabc123de…34abcd" (31 glyphs, 223.2) as one box; one line from 439.2 px.
  it('breaks where the arithmetic says: before the block number, or nowhere', async () => {
    const node = await caption();
    const words = 'the seal of the newest block,';
    const figures = `${WIDEST_BLOCK_NUMBER} · 0xabc123de…34abcd`;
    expect(textRunPx(words, node, 1280)).toBeCloseTo(208.8, 10);
    expect(textRunPx(figures, node, 1280)).toBeCloseTo(223.2, 10);
    const table = VIEWPORTS.map((viewport) => ({
      viewport,
      box: captionPx(viewport),
      lines: lines(node, viewport).map(text),
    }));
    const two = [words, figures];
    const one = [`${words} ${figures}`];
    expect(table).toEqual([
      { viewport: 320, box: 288, lines: two },
      { viewport: 360, box: 328, lines: two },
      { viewport: 390, box: 358, lines: two },
      { viewport: 640, box: 576, lines: one },
      { viewport: 1024, box: 832, lines: one },
      { viewport: 1280, box: 256, lines: two },
      { viewport: 1440, box: 256, lines: two },
    ]);
  });
});
