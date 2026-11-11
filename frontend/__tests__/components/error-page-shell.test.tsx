import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
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
import { ErrorPageShell } from '@/components/error-page-shell';

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
  // place of the brush talisman of its hash.
  it('with providers it presses the newest block’s seal behind the verse: the tip block’s stone, once', async () => {
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
    // decoration behind the text: hidden from the reader, who has the caption
    expect(seal).toHaveAttribute('aria-hidden', 'true');
    expect(seal).toHaveClass('error-seal');
    // one seal on the page, the stone and not the talisman
    expect(document.querySelectorAll('[data-seal]')).toHaveLength(1);
    expect(document.querySelector('[data-hash-seal]')).toBeNull();
    const drawn = seal.querySelector<HTMLElement>('[data-seal]')!;
    expect(seal.firstElementChild).toBe(drawn);
    expect(drawn).toHaveAttribute('data-seal', 'block');
    expect(drawn).toHaveAttribute('data-seal-form', 'page');
    expect(drawn).toHaveAttribute('data-seal-state', 'pressed');
    // the tip's block as /blocks/{n} serves it: its hash and its 5 transactions, drawn 469 px
    // square (the area of the 306 × 720 talisman), pressed −3° (the not-found board's hand)
    const id: SealId = { kind: 'block', hash: TIP_HASH, txs: 5 };
    expect(drawn.innerHTML).toBe(
      parsed(
        drawSeal(id, { form: 'page', height: 469, state: 'pressed', ...SEAL_INKS, tilt: -3 }).svg
      )
    );
    const svg = drawn.querySelector('svg')!;
    expect(svg).toHaveAttribute('aria-label', 'Block seal 0xabc123de, 5 transactions');
    expect(svg).toHaveAttribute('width', '469');
    expect(svg).toHaveAttribute('height', '469');
    // in the cinnabar mark, pressed a little askew; faint is the CSS's (--m-seal-press)
    expect(drawn.innerHTML).toContain('var(--c-mark)');
    expect(drawn.innerHTML).not.toContain('var(--c-rouge)');
    expect(svg.getAttribute('style')).toBe('transform: rotate(-3deg)');
    // the seal sits in main, behind the verse, the first screen's height (globals.css)
    expect(screen.getByRole('main')).toContainElement(seal);
    expect(screen.getByRole('main')).toHaveClass('isolate');
    // the caption names the block, as a link, and its hash
    const caption = document.querySelector('[data-seal-caption]') as HTMLElement;
    expect(caption).toHaveTextContent(
      'behind: the seal of the newest block, #1,000,000 · 0xabc123de...1234abcd'
    );
    expect(within(caption).getByRole('link', { name: '#1,000,000' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/1000000'
    );
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

  // The stone takes about the area the brush talisman took: that one was 11.94 × 28.14 units, as
  // tall as main's box (a screen at most); a square of its area is 0.651 of its height.
  it('sizes the stone from the first screen: 0.65 of the talisman’s height, never past the gutters', () => {
    const css = readFileSync(resolve(__dirname, '../../app/globals.css'), 'utf-8');
    const rule = (selector: string) => {
      const at = css.indexOf(`\n${selector} {`);
      expect(at).toBeGreaterThan(-1);
      return css.slice(at, css.indexOf('}', at));
    };
    expect(Math.sqrt(11.94 / 28.14)).toBeCloseTo(0.65, 2);
    expect(rule('.error-seal > [data-seal]')).toContain(
      'height: min(65%, 65svh, calc(100vw - 32px));'
    );
    expect(rule('.error-seal')).toContain('opacity: var(--m-seal-press);');
    expect(rule('.error-seal svg')).toContain('width: auto;');
    // the talisman's rule is gone with it
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
