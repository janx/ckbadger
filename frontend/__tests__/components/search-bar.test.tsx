import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '../utils/test-utils';
import { api } from '@/lib/api';

const pushMock = vi.hoisted(() => vi.fn());

vi.mock('next/navigation', () => ({
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

  it('renders shortcut hints in home variant', () => {
    render(<SearchBar variant="home" />);
    const input = screen.getByPlaceholderText('Search block / tx / address / cell ...');

    expect(input).toBeInTheDocument();
    expect(screen.getByText('/')).toBeInTheDocument();
    expect(screen.getByText('?')).toBeInTheDocument();
    expect(screen.queryByTestId('home-search-focus-glow')).not.toBeInTheDocument();
    expect(screen.queryByTestId('home-search-focus-border-scan')).not.toBeInTheDocument();

    fireEvent.focus(input);
    expect(screen.getByTestId('home-search-focus-glow')).toBeInTheDocument();
    const borderScan = screen.getByTestId('home-search-focus-border-scan');
    expect(borderScan).toBeInTheDocument();
    expect(borderScan.querySelectorAll(':scope > span')).toHaveLength(1);
    expect(screen.queryByRole('button', { name: 'Search' })).not.toBeInTheDocument();
  });

  it('does not render shortcut hints in compact variant', () => {
    render(<SearchBar variant="compact" />);

    expect(screen.getByPlaceholderText('Search blocks, txs...')).toBeInTheDocument();
    expect(screen.queryByText('?')).not.toBeInTheDocument();
    expect(screen.queryByTestId('home-search-focus-glow')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Search' })).not.toBeInTheDocument();
  });

  it('renders semantic icons for different search result types', async () => {
    render(<SearchBar />);

    const input = screen.getByPlaceholderText('Block, tx hash, address...');
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

    const input = screen.getByPlaceholderText('Block, tx hash, address...');
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

    const input = screen.getByPlaceholderText('Block, tx hash, address...');
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

    const input = screen.getByPlaceholderText('Block, tx hash, address...');
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
});
