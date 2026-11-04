import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor } from '../utils/test-utils';
import { api } from '@/lib/api';

const SHARED_PLACEHOLDER = 'Search block / tx / address / cell ...';

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
  beforeEach(() => {
    vi.clearAllMocks();
    pushMock.mockReset();
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
        'focus:shadow-[0_0_0_1px_rgba(46,219,163,0.12),0_0_12px_rgba(46,219,163,0.1)]',
      ],
      default: [
        'focus:border-jade/50',
        'focus:shadow-[0_0_0_1px_rgba(46,219,163,0.12),0_0_12px_rgba(46,219,163,0.1)]',
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
      'focus:shadow-[0_0_0_1px_rgba(46,219,163,0.12),0_0_12px_rgba(46,219,163,0.1)]'
    );
    expect(input).not.toHaveClass('text-transparent');
    // the prompt glyph is decoration, before the text (the field's padding makes room for it)
    const prompt = screen.getByTestId('search-prompt');
    expect(prompt).toHaveTextContent('>');
    expect(prompt).toHaveAttribute('aria-hidden', 'true');
    expect(prompt).toHaveClass('text-jade', 'pointer-events-none');
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
    expect(screen.getByTestId('search-prompt')).toHaveTextContent('>');
    let input = screen.getByPlaceholderText(SHARED_PLACEHOLDER);
    expect(input).toHaveClass('pl-7', 'focus:border-jade/50');
    fireEvent.focus(input);
    expect(screen.getByTestId('search-cursor')).toHaveClass('cursor');
    rerender(<SearchBar variant="home" />);
    expect(screen.getByTestId('search-prompt')).toHaveTextContent('>');
    input = screen.getByPlaceholderText(SHARED_PLACEHOLDER);
    expect(input).toHaveClass('pl-8');
    fireEvent.focus(input);
    expect(screen.getByTestId('search-cursor')).toHaveClass('cursor');
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
      expect(screen.getByText('No results found')).toBeInTheDocument();
    });

    fireEvent.submit(input.closest('form')!);
    expect(pushMock).not.toHaveBeenCalled();
    expect(screen.getByText('No matches found.')).toBeInTheDocument();
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
});
