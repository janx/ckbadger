import { MemoryRouter } from 'react-router-dom';
import { describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { NotFoundPage } from '@/components/not-found-page';
import { useNotFoundEvidence } from '@/lib/not-found-evidence';

vi.mock('@/lib/not-found-evidence', () => ({
  useNotFoundEvidence: vi.fn(() => ({ status: 'idle' })),
}));
vi.mock('@/src/navigation', async () => await vi.importActual('@/src/navigation'));

describe('NotFoundPage', () => {
  it('keeps an unknown route useful without claiming an entity is missing', () => {
    render(
      <MemoryRouter initialEntries={['/mainnet/a-route-that-is-not-here']}>
        <NotFoundPage />
      </MemoryRouter>
    );

    expect(screen.getByRole('heading', { name: 'The road ends here.' })).toHaveStyle({
      fontFamily: 'var(--font-error-display)',
    });
    expect(screen.getByText(/a-route-that-is-not-here/)).toBeInTheDocument();
    expect(screen.getByText('This route does not lead to a page.')).toBeInTheDocument();
    expect(screen.queryByText(/searched every block|pool|cell_not_found/i)).not.toBeInTheDocument();
    const nav = screen.getByRole('navigation', { name: 'Find another route' });
    expect(within(nav).getByRole('link', { name: 'Back to the explorer →' })).toHaveAttribute(
      'href',
      '/mainnet'
    );
    expect(within(nav).getByRole('link', { name: 'Inventory' })).toHaveAttribute(
      'href',
      '/mainnet/inventory/tokens'
    );
    expect(screen.getByRole('main').parentElement).toHaveClass('overflow-x-hidden');
  });

  it('reports the exact checked endpoint and links a verified alternative', () => {
    vi.mocked(useNotFoundEvidence).mockReturnValueOnce({
      status: 'checked',
      evidence: {
        checkedEndpoint: '/transactions/0xabc',
        query: 'tx:0xabc',
        searchEndpoint: '/search',
        finding: 'The transaction endpoint returned 404. A block matched this exact hash.',
        alternative: {
          label: 'block #42',
          href: '/blocks/42',
          resultType: 'block',
          matchKind: 'exact_hash',
        },
      },
    });
    render(
      <MemoryRouter initialEntries={['/mainnet/tx/0xabc']}>
        <NotFoundPage
          entity={{
            kind: 'transaction',
            identifier: '0xabc',
            checkedEndpoint: '/transactions/0xabc',
          }}
          footer
        />
      </MemoryRouter>
    );

    expect(screen.getByRole('heading', { name: 'A name, not a transaction.' })).toBeInTheDocument();
    expect(screen.getByText('/transactions/0xabc')).toBeInTheDocument();
    expect(screen.getByText(/A block matched this exact hash/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Open block #42 →' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/42'
    );
    expect(screen.getAllByRole('contentinfo')).toHaveLength(1);
  });

  it('shows lookup failure as uncertainty, not a fabricated absence', () => {
    vi.mocked(useNotFoundEvidence).mockReturnValueOnce({
      status: 'error',
      error: 'search unavailable',
    });
    render(
      <NotFoundPage
        entity={{
          kind: 'transaction',
          identifier: '0x' + 'f'.repeat(64),
          checkedEndpoint: '/transactions/0x' + 'f'.repeat(64),
        }}
      />
    );
    expect(screen.getByText('The local evidence check could not complete.')).toBeInTheDocument();
    expect(screen.getByRole('status')).toHaveTextContent('search unavailable');
    expect(screen.getByText('0x' + 'f'.repeat(64))).toHaveClass('break-all');
  });
});
