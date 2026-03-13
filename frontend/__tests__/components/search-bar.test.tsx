import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '../utils/test-utils';
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

  it('updates the compact command-line preview as the query changes', () => {
    render(<SearchBar variant="compact" />);

    const input = screen.getByPlaceholderText(SHARED_PLACEHOLDER);
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: '0xabc123' } });

    const commandText = screen.getByTestId('compact-search-command-text');
    const commandCursor = screen.getByTestId('compact-search-cursor');

    expect(commandText).toHaveTextContent('0xabc123');
    expect(commandText).not.toHaveTextContent(SHARED_PLACEHOLDER);
    expect(commandText).not.toHaveTextContent('[/?]');
    expect(commandCursor).toBeInTheDocument();
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
