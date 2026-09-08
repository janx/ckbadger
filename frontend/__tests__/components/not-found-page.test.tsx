import { describe, it, expect } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { NotFoundPage } from '@/components/not-found-page';

describe('NotFoundPage', () => {
  it('renders 404 with poetry and terminal error', () => {
    render(<NotFoundPage />);

    // 404 characters present (two "4" spans + one "0" span)
    expect(screen.getAllByText('4', { exact: true })).toHaveLength(2);
    expect(screen.getByText('0', { exact: true })).toBeInTheDocument();

    // Poetry lines
    expect(
      screen.getByText('some common knowledge has dissolved into the void')
    ).toBeInTheDocument();
    expect(screen.getByText('yet more is crystallizing from the chain')).toBeInTheDocument();

    // Terminal error line
    expect(screen.getByText(/cell_not_found/)).toBeInTheDocument();

    // Header nav links still present
    expect(screen.getByRole('link', { name: 'DAO' })).toHaveAttribute('href', '/mainnet/dao');
    expect(screen.getByRole('link', { name: 'Tokens' })).toHaveAttribute(
      'href',
      '/mainnet/inventory/tokens'
    );
    expect(screen.getByRole('link', { name: 'Scripts' })).toHaveAttribute(
      'href',
      '/mainnet/scripts'
    );
    expect(screen.getByRole('link', { name: 'Charts' })).toHaveAttribute('href', '/mainnet/charts');

    // No debug UI
    expect(screen.queryByText('Ocean Tuning')).not.toBeInTheDocument();
    expect(screen.queryByText('Track Blocks')).not.toBeInTheDocument();
  });

  it('omits the design and coding attribution from its footer', () => {
    render(<NotFoundPage />);

    expect(screen.getByRole('main')).not.toHaveTextContent(
      /designed by|coded by|busyforking|Claude|Codex/i
    );
    expect(screen.getByRole('link', { name: 'Hardforks' })).toHaveAttribute(
      'href',
      '/mainnet/hardforks'
    );
    expect(screen.getByRole('link', { name: 'Fiber Dashboard' })).toHaveAttribute(
      'href',
      'https://dashboard.fiber.channel/'
    );
  });
});
