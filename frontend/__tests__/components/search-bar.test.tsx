import { readFileSync } from 'node:fs';
import { renderPaused } from '@/__tests__/utils/paused-read';
import { resolve } from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor } from '../utils/test-utils';
import { stubViewportFromMd } from '@/__tests__/utils/viewport';
import { api } from '@/lib/api';

/** U-01 / R2 (the boards' header, lib.js `header()`): lowercase, no dots. */
const SHARED_PLACEHOLDER = 'search block / tx / address / cell';
/** The placeholder of a field whose text box cannot hold the long one: 6 characters, 46.8 px. */
const PHONE_PLACEHOLDER = 'search';

/**
 * Lays the field out as a browser would (jsdom lays nothing out): every input `width` px wide
 * (its border box) with the compact field's insets, 1 px borders, 28 px of left padding (the prompt
 * square's) and `right` px of right padding (36 with the `/` key's room, from md; 12 without), so
 * its text box is width − 30 − right. The field reads it before paint; `resize` sets a new width
 * and has the ResizeObserver report it, as a browser does on a resize.
 */
function layOutField(width: number, right = 36) {
  let border = width;
  let report: () => void = () => {
    throw new Error('layOutField: no ResizeObserver was made');
  };
  class ResizeObserverStub {
    constructor(cb: ResizeObserverCallback) {
      report = () => cb([], this as unknown as ResizeObserver);
    }
    observe() {}
    unobserve() {}
    disconnect() {}
  }
  vi.stubGlobal('ResizeObserver', ResizeObserverStub);
  const rect = HTMLElement.prototype.getBoundingClientRect;
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
    this: HTMLElement
  ) {
    return this instanceof HTMLInputElement ? ({ width: border } as DOMRect) : rect.call(this);
  });
  const style = window.getComputedStyle.bind(window);
  vi.spyOn(window, 'getComputedStyle').mockImplementation((el, pseudo) =>
    el instanceof HTMLInputElement
      ? ({
          display: 'block',
          visibility: 'visible',
          borderLeftWidth: '1px',
          borderRightWidth: '1px',
          paddingLeft: '28px',
          paddingRight: `${right}px`,
        } as unknown as CSSStyleDeclaration)
      : style(el, pseudo)
  );
  return {
    resize(next: number) {
      border = next;
      act(() => report());
    },
  };
}

/** A desktop header's field: 460.8 px at 1280 (36vw), a 394.8 px text box. */
const DESKTOP_FIELD_PX = 460.8;

const pushMock = vi.hoisted(() => vi.fn());

vi.mock('@/src/navigation', () => ({
  useRouter: () => ({
    push: pushMock,
    replace: vi.fn(),
    back: vi.fn(),
    forward: vi.fn(),
    refresh: vi.fn(),
    prefetch: vi.fn(),
  }),
  useSearchParams: () => new URLSearchParams(),
  usePathname: () => '/',
  useParams: () => ({}),
}));

vi.mock('@/lib/api', () => ({
  api: {
    search: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));

import { SearchBar } from '@/components/search-bar';

describe('SearchBar', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  beforeEach(() => {
    vi.clearAllMocks();
    pushMock.mockReset();
    // the placeholder follows the measured field: unless a test says otherwise, a desktop one
    layOutField(DESKTOP_FIELD_PX);
    vi.mocked(api.search).mockResolvedValue({
      query: '12',
      results: [
        { resultType: 'block', id: '12', label: 'Block #12', url: '/blocks/12' },
        {
          resultType: 'transaction',
          id: '0xabc',
          label: 'Transaction 0xabc',
          url: '/tx/0xabc',
        },
        { resultType: 'address', id: 'ckb1xyz', label: 'Address ckb1xyz', url: '/address/ckb1xyz' },
        { resultType: 'cell', id: '0xdef-0', label: 'Cell 0xdef:0', url: '/cell/0xdef-0' },
      ],
    });
  });

  it('uses the same placeholder copy across variants', () => {
    const { rerender } = render(<SearchBar />);

    expect(screen.getByPlaceholderText(SHARED_PLACEHOLDER)).toBeInTheDocument();

    rerender(<SearchBar variant="compact" />);
    expect(screen.getByPlaceholderText(SHARED_PLACEHOLDER)).toBeInTheDocument();

    rerender(<SearchBar variant="home" />);
    expect(screen.getByPlaceholderText(SHARED_PLACEHOLDER)).toBeInTheDocument();
  });

  // PA-04 (UI review 2026-11-14): below md the header's field is what the row leaves, 180 px at
  // 360 (326 px of content less the 90 px wordmark, the 40 px menu button and two 8 px gaps), and
  // the full placeholder (38 characters at 13 px, 296.4 px) was cut mid-word at a different place
  // on every page. The phone's 180 px field (a 138 px text box) carries the short placeholder,
  // and no "/" key is drawn: a touch layout has no keyboard to hint at, so the text keeps the room
  // the key took.
  it('gives the phone field a short placeholder and no "/" key below md (PA-04)', () => {
    vi.restoreAllMocks();
    layOutField(180, 12);
    const { unmount } = render(<SearchBar variant="compact" />);
    const input = screen.getByPlaceholderText(PHONE_PLACEHOLDER);
    expect(screen.queryByPlaceholderText(SHARED_PLACEHOLDER)).toBeNull();
    // the key's 36 px of right padding only where the key is drawn
    expect(input).toHaveClass('pr-3', 'md:pr-9');
    expect(input).not.toHaveClass('pr-9');
    const hints = screen.getByTestId('search-shortcut-hints');
    expect(hints).toHaveClass('hidden', 'md:flex');
    expect(hints).not.toHaveClass('flex');
    // the block cursor's track ends where the text does
    fireEvent.focus(input);
    expect(screen.getByTestId('search-cursor-track')).toHaveClass('right-3', 'md:right-9');
    unmount();
    // the home field's keys follow the same breakpoint
    render(<SearchBar variant="home" />);
    expect(screen.getByPlaceholderText(PHONE_PLACEHOLDER)).toBeInTheDocument();
    expect(screen.getByTestId('search-shortcut-hints')).toHaveClass('hidden', 'md:flex');
    expect(screen.getByTestId('search-shortcut-hints')).not.toHaveClass('sm:flex');
  });

  // U-01 (UI review 2026-11-14), R2: the boards' field (lib.js `.search .sq`) prompts with a 7 px
  // square in the cinnabar mark, the wordmark's and the headings' square one pixel smaller; the
  // build drew a celadon ">". The square is decoration (aria-hidden, no text), centred on the
  // field's height, and the typed text starts after it at the board's 9 px gap: the square's left
  // (12 px, 16 px in the home field) + 7 + 9 = 28 px (pl-7), 32 px (pl-8), the cursor track too.
  it('prompts with the 7 px cinnabar square, the typed text 9 px after it (U-01)', () => {
    const css = readFileSync(resolve(__dirname, '../../app/globals.css'), 'utf-8');
    const rule = css.match(/\.search-square\s*\{([^}]*)\}/);
    expect(rule, '.search-square in globals.css').not.toBeNull();
    expect(rule![1]).toContain('width: 7px;');
    expect(rule![1]).toContain('height: 7px;');
    expect(rule![1]).toContain('background: var(--c-mark);');
    expect(rule![1]).not.toMatch(/border-radius|box-shadow/);
    for (const [variant, left, inset] of [
      ['compact', 'left-3', 'pl-7'],
      ['default', 'left-3', 'pl-7'],
      ['home', 'left-4', 'pl-8'],
    ] as const) {
      const { unmount } = render(<SearchBar variant={variant} />);
      const prompt = screen.getByTestId('search-prompt');
      expect(prompt, variant).toHaveAttribute('aria-hidden', 'true');
      expect(prompt, variant).toBeEmptyDOMElement();
      expect(prompt, variant).not.toHaveTextContent('>');
      expect(prompt, variant).not.toHaveClass('text-jade');
      expect(prompt, variant).toHaveClass(
        'search-square',
        'absolute',
        left,
        'top-1/2',
        '-translate-y-1/2'
      );
      const input = screen.getByPlaceholderText(SHARED_PLACEHOLDER);
      expect(input, variant).toHaveClass(inset);
      fireEvent.focus(input);
      expect(screen.getByTestId('search-cursor-track'), variant).toHaveClass(
        inset.replace('pl-', 'left-')
      );
      unmount();
    }
  });

  it('keeps the full placeholder in a desktop field, and the "/" key from md up (PA-04)', () => {
    render(<SearchBar variant="compact" />);
    expect(screen.getByPlaceholderText(SHARED_PLACEHOLDER)).toBeInTheDocument();
    expect(screen.getByTestId('search-shortcut-key-slash')).toHaveTextContent('/');
    expect(screen.getByTestId('search-shortcut-hints')).toHaveClass('hidden', 'md:flex');
  });

  // Follow-up to PA-04 (RESPONSIVE_LAYOUT principle 4, "the measured width decides, not the
  // breakpoint"): from md the long placeholder was drawn at every width, and the header's field
  // holds about 105 px of text at 768–1023 (and less than the placeholder at 1024 beside the
  // network switcher), so it was cut mid-word there. The field measures its text box (the input's
  // content box, inside its borders and paddings) before paint and keeps it current, and draws the
  // long placeholder only when the box holds all of it: 34 characters at 13 px, 0.6 em each, 265.2
  // px; in the header's field from md (1 + 28 + 36 + 1 px of insets) that is 331.2 px wide.
  describe('chooses its placeholder by the measured text box, not a breakpoint', () => {
    it('draws the short one just under 265.2 px of text box and the long one just over', () => {
      vi.restoreAllMocks();
      layOutField(330.5); // a 264.5 px text box
      const { unmount } = render(<SearchBar variant="compact" />);
      expect(screen.getByPlaceholderText(PHONE_PLACEHOLDER)).toBeInTheDocument();
      unmount();
      vi.restoreAllMocks();
      layOutField(332); // 266 px
      render(<SearchBar variant="compact" />);
      expect(screen.getByPlaceholderText(SHARED_PLACEHOLDER)).toBeInTheDocument();
    });

    it('draws the short one in a narrow field at an md-wide viewport', () => {
      vi.restoreAllMocks();
      stubViewportFromMd(true);
      layOutField(169); // the field at 768–1023: 736 px of row less 567 px of the rest
      render(<SearchBar variant="compact" />);
      expect(screen.getByPlaceholderText(PHONE_PLACEHOLDER)).toBeInTheDocument();
      expect(screen.queryByPlaceholderText(SHARED_PLACEHOLDER)).toBeNull();
    });

    it('draws the long one in a wide field below md', () => {
      vi.restoreAllMocks();
      stubViewportFromMd(false);
      layOutField(400, 12);
      render(<SearchBar variant="compact" />);
      expect(screen.getByPlaceholderText(SHARED_PLACEHOLDER)).toBeInTheDocument();
    });

    it('draws the short one until the field is laid out, so a narrow field never shows the long one', () => {
      vi.restoreAllMocks();
      render(<SearchBar variant="compact" />);
      expect(screen.getByPlaceholderText(PHONE_PLACEHOLDER)).toBeInTheDocument();
    });

    it('follows the field when it is resized', () => {
      vi.restoreAllMocks();
      const field = layOutField(DESKTOP_FIELD_PX);
      render(<SearchBar variant="compact" />);
      expect(screen.getByPlaceholderText(SHARED_PLACEHOLDER)).toBeInTheDocument();
      field.resize(281); // 1024 beside the network switcher: a 215 px text box
      expect(screen.getByPlaceholderText(PHONE_PLACEHOLDER)).toBeInTheDocument();
      field.resize(368.6); // 1024 with one network: 302.6 px
      expect(screen.getByPlaceholderText(SHARED_PLACEHOLDER)).toBeInTheDocument();
    });
  });

  it('renders shortcut hints in home variant', () => {
    render(<SearchBar variant="home" />);
    expect(screen.getByTestId('search-shortcut-hints')).toBeInTheDocument();
    expect(screen.getByTestId('search-shortcut-key-slash')).toHaveTextContent('/');
    expect(screen.getByTestId('search-shortcut-key-question')).toHaveTextContent('?');
    expect(screen.queryByRole('button', { name: 'Search' })).not.toBeInTheDocument();
  });

  // Visual finish V9 follow-up (one focus ring per control): every variant of the field designs
  // its own focus — the jade-tinted border with its soft ring (compact / default), the jade border
  // and 2 px ring (home) — so it opts out of the global jade outline.
  it('draws one focus line in every variant: its own jade border and ring, no global outline', () => {
    const own = {
      compact: [
        'focus:border-jade/50',
        'focus:shadow-[0_0_0_1px_rgb(var(--rgb-jade)/0.12),0_0_12px_rgb(var(--rgb-jade)/calc(0.1*var(--m-glow)))]',
      ],
      default: [
        'focus:border-jade/50',
        'focus:shadow-[0_0_0_1px_rgb(var(--rgb-jade)/0.12),0_0_12px_rgb(var(--rgb-jade)/calc(0.1*var(--m-glow)))]',
      ],
      home: ['focus:border-jade', 'focus:ring-2', 'focus:ring-jade/25'],
    } as const;
    for (const variant of ['compact', 'default', 'home'] as const) {
      const { unmount } = render(<SearchBar variant={variant} />);
      const input = screen.getByPlaceholderText(SHARED_PLACEHOLDER);
      expect(input).toHaveClass('focus-visible:outline-none', ...own[variant]);
      expect(input).not.toHaveClass('outline-none', 'focus:outline-none');
      unmount();
    }
  });

  // TX-03 (Main.dc.html header): the compact search is a 36 px rounded field with one "/" key,
  // and the typed text is the input's own (no simulated command line). Review 2026-11-03, item 5,
  // move 4 ("the terminal speaks") gives the field its prompt back by design: a jade ">" before
  // the text, and — only while focused — a block cursor after the typed text with a jade-tinted
  // border; the earlier "no prompt, no caret" reading of TX-03 is superseded by that approval.
  it('draws the compact search as a prompt: ">", the typed text itself, a block cursor on focus and one "/" key', () => {
    render(<SearchBar variant="compact" />);
    const input = screen.getByPlaceholderText(SHARED_PLACEHOLDER);
    // material pass v2 (move 2): the field is a well (`.well`: dark inset glass, the row-rule
    // border; its corners stay the board's 6 px through --well-radius)
    expect(input).toHaveClass('well', '[--well-radius:6px]', 'h-9');
    expect(input).not.toHaveClass('bg-base-surface', 'rounded', 'border-base-border');
    expect(input).toHaveClass(
      // the jade-tinted focus border and its soft ring
      'focus:border-jade/50',
      'focus:shadow-[0_0_0_1px_rgb(var(--rgb-jade)/0.12),0_0_12px_rgb(var(--rgb-jade)/calc(0.1*var(--m-glow)))]'
    );
    expect(input).not.toHaveClass('text-transparent');
    // the prompt is decoration, before the text (the field's padding makes room for it); since
    // U-01 it is the cinnabar square, not a celadon ">"
    const prompt = screen.getByTestId('search-prompt');
    expect(prompt).toBeEmptyDOMElement();
    expect(prompt).toHaveAttribute('aria-hidden', 'true');
    expect(prompt).toHaveClass('search-square', 'pointer-events-none');
    expect(input).toHaveClass('pl-7');
    // no cursor while the field is idle, and the native caret is the browser's
    expect(screen.queryByTestId('search-cursor')).toBeNull();
    expect(input).not.toHaveClass('caret-transparent');
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: '0xabc123' } });
    expect(input).toHaveValue('0xabc123');
    // focused: the block cursor stands after the typed text, in place of the native caret
    const cursor = screen.getByTestId('search-cursor');
    expect(cursor).toHaveClass('cursor');
    expect(input).toHaveClass('caret-transparent');
    const track = screen.getByTestId('search-cursor-track');
    expect(track).toHaveAttribute('aria-hidden', 'true');
    expect(track.firstElementChild).toHaveClass('invisible');
    expect(track.firstElementChild).toHaveTextContent('0xabc123');
    expect(track.lastElementChild).toBe(cursor);
    // the cursor follows the caret: with the caret after "0xab" the mirror holds "0xab"
    (input as HTMLInputElement).setSelectionRange(4, 4);
    fireEvent.select(input);
    expect(track.firstElementChild).toHaveTextContent(/^0xab$/);
    // the "/" key stays; the "?" key lives in the footer
    expect(screen.getByTestId('search-shortcut-key-slash')).toHaveTextContent('/');
    expect(screen.queryByTestId('search-shortcut-key-question')).toBeNull();
    // blurred: the cursor goes, the native caret comes back
    fireEvent.blur(input);
    expect(screen.queryByTestId('search-cursor')).toBeNull();
    expect(input).not.toHaveClass('caret-transparent');
  });

  it('gives the default and home fields the same prompt and cursor (move 4)', () => {
    const { rerender } = render(<SearchBar />);
    expect(screen.getByTestId('search-prompt')).toHaveClass('search-square');
    let input = screen.getByPlaceholderText(SHARED_PLACEHOLDER);
    expect(input).toHaveClass('pl-7', 'focus:border-jade/50');
    fireEvent.focus(input);
    expect(screen.getByTestId('search-cursor')).toHaveClass('cursor');
    rerender(<SearchBar variant="home" />);
    expect(screen.getByTestId('search-prompt')).toHaveClass('search-square');
    input = screen.getByPlaceholderText(SHARED_PLACEHOLDER);
    expect(input).toHaveClass('pl-8');
    fireEvent.focus(input);
    expect(screen.getByTestId('search-cursor')).toHaveClass('cursor');
  });

  // Visual finish V11: the compact results' well, selected row and hover are named inks
  // (SEARCH_RESULT_COLORS), the hover through a CSS variable its class reads; no bracket hex.
  it('draws the compact results in their own well, the selected row lit, the hover through a variable', async () => {
    render(<SearchBar variant="compact" />);
    const input = screen.getByPlaceholderText(SHARED_PLACEHOLDER);
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: '12' } });
    const dropdown = await screen.findByTestId('search-results-dropdown');
    await waitFor(() => expect(screen.getByText('Block #12')).toBeInTheDocument());
    expect(dropdown).toHaveStyle({ backgroundColor: 'var(--c-search-panel)' });
    const row = (label: string) => screen.getByText(label).closest('button') as HTMLElement;
    expect(row('Block #12')).toHaveClass('text-text', 'hover:bg-[color:var(--row-hover)]');
    expect(row('Block #12').style.getPropertyValue('--row-hover')).toBe('var(--c-search-hover)');
    expect(row('Block #12').style.backgroundColor).toBe('');
    fireEvent.keyDown(input, { key: 'ArrowDown' });
    expect(row('Block #12')).toHaveClass('text-jade');
    expect(row('Block #12')).toHaveStyle({ backgroundColor: 'var(--c-search-selected)' });
    expect(row('Transaction 0xabc').style.getPropertyValue('--row-hover')).toBe(
      'var(--c-search-hover)'
    );
    expect(dropdown.outerHTML).not.toMatch(/(text|bg|border)-\[#/);
  });

  it('keeps the default results on theme classes, with no inline colour', async () => {
    render(<SearchBar />);
    const input = screen.getByPlaceholderText(SHARED_PLACEHOLDER);
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: '12' } });
    const dropdown = await screen.findByTestId('search-results-dropdown');
    await waitFor(() => expect(screen.getByText('Block #12')).toBeInTheDocument());
    expect(dropdown).toHaveClass('bg-base-surface');
    expect(dropdown.getAttribute('style')).toBeNull();
    expect(
      (screen.getByText('Block #12').closest('button') as HTMLElement).getAttribute('style')
    ).toBeNull();
  });

  it('renders semantic icons for different search result types', async () => {
    render(<SearchBar />);

    const input = screen.getByPlaceholderText(SHARED_PLACEHOLDER);
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: '12' } });

    await waitFor(() => {
      expect(screen.getByTestId('search-result-icon-block')).toBeInTheDocument();
      expect(screen.getByTestId('search-result-icon-transaction')).toBeInTheDocument();
      expect(screen.getByTestId('search-result-icon-address')).toBeInTheDocument();
      expect(screen.getByTestId('search-result-icon-cell')).toBeInTheDocument();
    });
  });

  it('does not auto-navigate when multiple matches are returned on submit', async () => {
    render(<SearchBar />);

    const input = screen.getByPlaceholderText(SHARED_PLACEHOLDER);
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: '12' } });

    await waitFor(() => {
      expect(screen.getByText('Block #12')).toBeInTheDocument();
    });

    fireEvent.submit(input.closest('form')!);

    expect(pushMock).not.toHaveBeenCalled();
    expect(
      screen.getByText('Multiple matches found. Please choose one result.')
    ).toBeInTheDocument();
  });

  it('navigates immediately when exactly one result is returned on submit', async () => {
    vi.mocked(api.search).mockResolvedValueOnce({
      query: 'alpha',
      results: [{ resultType: 'script', id: '0xabc', label: 'Script Alpha', url: '/script/0xabc' }],
    });

    render(<SearchBar />);

    const input = screen.getByPlaceholderText(SHARED_PLACEHOLDER);
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: 'alpha' } });

    await waitFor(() => {
      expect(screen.getByText('Script Alpha')).toBeInTheDocument();
    });

    fireEvent.submit(input.closest('form')!);
    expect(pushMock).toHaveBeenCalledWith('/script/0xabc');
  });

  /// Every valid CKB address now comes back as exactly one result, whatever
  /// the chain knows about it, so pasting one always lands on its page —
  /// including a brand-new address whose only transaction is still in the
  /// node's pool. The bar itself needs no special case for that.
  it('navigates to an address whose only presence is the tx pool', async () => {
    const address =
      'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqdnnw7qkdnnclfkg59uzn8umtfd2kwxceqxwquc4';
    vi.mocked(api.search).mockResolvedValueOnce({
      query: address,
      results: [
        {
          resultType: 'address',
          id: address,
          label: 'Address (0 cells, 1 pending)',
          url: `/address/${address}`,
        },
      ],
    });

    render(<SearchBar />);

    const input = screen.getByPlaceholderText(SHARED_PLACEHOLDER);
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: address } });

    await waitFor(() => {
      expect(screen.getByText('Address (0 cells, 1 pending)')).toBeInTheDocument();
    });

    fireEvent.submit(input.closest('form')!);
    expect(pushMock).toHaveBeenCalledWith(`/address/${address}`);
  });

  it('shows no-match feedback instead of navigating on empty result set', async () => {
    vi.mocked(api.search).mockResolvedValueOnce({
      query: 'not-found',
      results: [],
    });

    render(<SearchBar />);

    const input = screen.getByPlaceholderText(SHARED_PLACEHOLDER);
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: 'not-found' } });

    await waitFor(() => {
      expect(api.search).toHaveBeenCalledWith('not-found');
    });
    await waitFor(() => {
      expect(screen.getByText('No results.')).toBeInTheDocument();
    });

    fireEvent.submit(input.closest('form')!);
    expect(pushMock).not.toHaveBeenCalled();
    expect(screen.getByText('No matches.')).toBeInTheDocument();
  });

  it('prefers exact block hash result on submit when hash query has mixed results', async () => {
    const hash = `0x${'a'.repeat(64)}`;
    vi.mocked(api.search).mockImplementation(async (query: string) => {
      if (query !== hash) {
        return { query, results: [] };
      }

      return {
        query: hash,
        results: [
          {
            resultType: 'transaction',
            id: hash,
            label: `Transaction ${hash}`,
            url: `/tx/${hash}`,
            matchKind: 'exact_hash',
          },
          {
            resultType: 'block',
            id: '123',
            label: 'Block #123',
            url: `/blocks/${hash}`,
            matchKind: 'exact_hash',
          },
        ],
      };
    });

    render(<SearchBar />);

    const input = screen.getByPlaceholderText(SHARED_PLACEHOLDER);
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: hash } });

    await waitFor(() => {
      expect(api.search).toHaveBeenCalledWith(hash);
    });
    await waitFor(() => {
      expect(screen.getByText('Block #123')).toBeInTheDocument();
    });

    fireEvent.submit(input.closest('form')!);
    expect(pushMock).toHaveBeenCalledWith(`/blocks/${hash}`);
  });

  // A1 review fix, round 2: a search paused offline, or in a hidden tab between retries, is still
  // awaited: the field says it is searching, never "No results." / "No matches.", and a real
  // empty answer still says so once back online.
  it('says it is searching while its read is paused, then the true empty answer once back online', async () => {
    vi.mocked(api.search).mockResolvedValue({ query: 'not-found', results: [] });
    const { resume } = renderPaused(<SearchBar />);
    const input = screen.getByPlaceholderText(SHARED_PLACEHOLDER);
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: 'not-found' } });
    expect(await screen.findByText('Searching…')).toBeInTheDocument();
    expect(screen.queryByText('No results.')).toBeNull();
    fireEvent.submit(input.closest('form')!);
    expect(screen.queryByText('No matches.')).toBeNull();
    expect(pushMock).not.toHaveBeenCalled();
    resume();
    expect(await screen.findByText('No results.')).toBeInTheDocument();
  });
});
