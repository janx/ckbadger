import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '../utils/test-utils';
import { SearchBar } from '@/components/search-bar';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    search: vi.fn(),
  },
}));

describe('SearchBar', () => {
  beforeEach(() => {
    vi.clearAllMocks();
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
});
