import { MemoryRouter } from 'react-router-dom';
import { describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { entityHash, NotFoundPage } from '@/components/not-found-page';
import { useNotFoundEvidence } from '@/lib/not-found-evidence';

vi.mock('@/lib/not-found-evidence', () => ({
  useNotFoundEvidence: vi.fn(() => ({ status: 'idle' })),
}));
vi.mock('@/src/navigation', async () => await vi.importActual('@/src/navigation'));

const HASH = '0x4222e86425cf6f48b95cd69a4aab946a91a2598d404411177b3003042f0aca85';

describe('NotFoundPage', () => {
  it('a route that names no page: the thought, the honest line, the ways on — and no claim about an entity', () => {
    render(
      <MemoryRouter initialEntries={['/mainnet/a-route-that-is-not-here']}>
        <NotFoundPage />
      </MemoryRouter>
    );

    expect(
      screen.getByRole('heading', { level: 1, name: 'all that is was worked for. this was not.' })
    ).toHaveClass('sign');
    expect(screen.getByText('Not found · 404')).toHaveClass('eyebrow');
    expect(screen.getByText(/a-route-that-is-not-here/)).toBeInTheDocument();
    expect(screen.getByText('no page at this path.')).toBeInTheDocument();
    expect(screen.getByText(/the chain was not asked · nothing on it is lost/)).toBeInTheDocument();
    expect(screen.queryByText(/searched every block|pool|cell_not_found/i)).not.toBeInTheDocument();
    const nav = screen.getByRole('navigation', { name: 'Find another route' });
    expect(within(nav).getByRole('link', { name: '← back to the explorer' })).toHaveAttribute(
      'href',
      '/mainnet'
    );
    expect(within(nav).getByRole('link', { name: 'browse blocks' })).toHaveAttribute(
      'href',
      '/mainnet/blocks'
    );
    expect(within(nav).getByRole('link', { name: 'inventory' })).toHaveAttribute(
      'href',
      '/mainnet/inventory/tokens'
    );
    expect(screen.getByRole('main').parentElement).toHaveClass('overflow-x-hidden');
    // the night waits for the first block: no hash of its own, so no grid in the CSS night either
    expect(document.querySelector('.error-night')).toHaveAttribute('data-night', 'failed');
    expect(screen.getByTestId('css-night').querySelector('.error-night-bits')).toBeNull();
    expect(screen.queryByRole('contentinfo')).toBeNull();
  });

  it('an entity known as something else: the hash is a fact, and the verified alternative is the way on', () => {
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
          standalone
        />
      </MemoryRouter>
    );

    expect(
      screen.getByRole('heading', { name: 'a hash is a fact. a name, a guess.' })
    ).toBeInTheDocument();
    expect(screen.getByText('no transaction at this address.')).toBeInTheDocument();
    expect(screen.getByText('/transactions/0xabc')).toBeInTheDocument();
    expect(screen.getByText(/A block matched this exact hash/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: '→ open block #42' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/42'
    );
    expect(screen.getByRole('link', { name: '← transactions' })).toHaveAttribute(
      'href',
      '/mainnet/transactions'
    );
    expect(screen.getAllByRole('contentinfo')).toHaveLength(1);
  });

  it('preserves and wraps a full endpoint in a verified finding, and pours the asked hash as the first grid', () => {
    const endpoint = `/transactions/${HASH}/detail`;
    const finding = `The ${endpoint} entity request returned HTTP 404; /search identified this exact hash as a block.`;
    vi.mocked(useNotFoundEvidence).mockReturnValueOnce({
      status: 'checked',
      evidence: {
        checkedEndpoint: endpoint,
        query: `tx:${HASH}`,
        searchEndpoint: '/search',
        finding,
      },
    });

    render(
      <MemoryRouter initialEntries={[`/mainnet/tx/${HASH}`]}>
        <NotFoundPage
          entity={{ kind: 'transaction', identifier: HASH, checkedEndpoint: endpoint }}
        />
      </MemoryRouter>
    );

    const findingText = screen.getByText(finding);
    expect(findingText).toHaveTextContent(endpoint);
    expect(findingText).toHaveClass('break-all');
    // nothing answered: the sign is the absence that is not denial
    expect(
      screen.getByRole('heading', { name: 'known to some, perhaps. to all, not yet.' })
    ).toBeInTheDocument();
    // jsdom has no WebGL2: the CSS night shows the asked hash, bit for bit
    expect(screen.getByTestId('css-night').querySelectorAll('.error-night-bits i')).toHaveLength(
      256
    );
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
    // the header's vitals have a status of their own: the facts' is inside main
    expect(within(screen.getByRole('main')).getByRole('status')).toHaveTextContent(
      'the separate /search check could not complete: search unavailable'
    );
    expect(screen.getByText('/transactions/0x' + 'f'.repeat(64))).toBeInTheDocument();
    expect(screen.getByText('0x' + 'f'.repeat(64))).toHaveClass('break-all');
  });

  it('the hash a page is about: a 32-byte hash, an out point’s transaction, or nothing', () => {
    expect(entityHash(undefined)).toBeNull();
    expect(
      entityHash({
        kind: 'transaction',
        identifier: '0x' + HASH.slice(2).toUpperCase(),
        checkedEndpoint: '/x',
      })
    ).toBe(HASH);
    expect(entityHash({ kind: 'cell', identifier: `${HASH}-3`, checkedEndpoint: '/x' })).toBe(HASH);
    expect(
      entityHash({
        kind: 'address',
        identifier:
          'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq2qf8keemy2p5uu0g0gn8cd4ju23s5269qk8rk4r',
        checkedEndpoint: '/x',
      })
    ).toBeNull();
  });
});
