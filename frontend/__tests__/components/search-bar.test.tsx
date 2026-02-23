import { describe, expect, it } from 'vitest';
import { fireEvent, render, screen } from '../utils/test-utils';
import { SearchBar } from '@/components/search-bar';

describe('SearchBar', () => {
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
});
