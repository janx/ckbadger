import { MemoryRouter } from 'react-router-dom';
import { describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { entityVerse, NotFoundPage, ROUTE_VERSE } from '@/components/not-found-page';
import { useNotFoundEvidence } from '@/lib/not-found-evidence';

vi.mock('@/lib/not-found-evidence', () => ({
  useNotFoundEvidence: vi.fn(() => ({ status: 'idle' })),
}));
vi.mock('@/src/navigation', async () => await vi.importActual('@/src/navigation'));

const HASH = '0x4222e86425cf6f48b95cd69a4aab946a91a2598d404411177b3003042f0aca85';
/** The tip's block hash the MSW `/blocks/:id` answers with (the vitals' tip is #1,000,000). */
const TIP_HASH = '0xabc123def456789012345678901234567890123456789012345678901234abcd';
const ROUTE_NAME =
  "no page at this path. the chain was not asked; nothing on it is lost. honey badger don't care.";

/** The verse's lines as rendered, in order. */
function verseLines(h1: HTMLElement): string[] {
  return Array.from(h1.querySelectorAll('[data-comment-line]'), (l) => l.textContent ?? '');
}

describe('NotFoundPage', () => {
  it('a route that names no page: the three lines of the verse, the ways on — and no claim about an entity', async () => {
    render(
      <MemoryRouter initialEntries={['/mainnet/a-route-that-is-not-here']}>
        <NotFoundPage />
      </MemoryRouter>
    );

    // rubbing × Song print, phase 4: the title is the verse, in the voice, at the display size
    const h1 = screen.getByRole('heading', { level: 1, name: ROUTE_NAME });
    expect(h1).toHaveClass('comment', 'comment-lg');
    expect(verseLines(h1)).toEqual([
      'no page at this path.',
      'the chain was not asked; nothing on it is lost.',
      "honey badger don't care.",
    ]);
    expect(verseLines(h1)).toEqual([...ROUTE_VERSE]);
    expect(screen.getByText('Not found · 404')).toHaveClass('eyebrow');
    expect(screen.getByText(/a-route-that-is-not-here/)).toBeInTheDocument();
    // the route's facts are the ways on: the verse already says what happened
    expect(
      screen.queryByText(/searched every block|pool|cell_not_found|checked/i)
    ).not.toBeInTheDocument();
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
    expect(screen.queryByRole('contentinfo')).toBeNull();
    // behind the verse, the newest block's seal: the vitals' tip, its hash read from /blocks/{n}
    const seal = await screen.findByTestId('error-seal');
    expect(seal.querySelector('[data-hash-seal="brush"] svg')).toHaveAttribute(
      'aria-label',
      `Hash seal ${TIP_HASH.slice(0, 10)}`
    );
    // no WebGL, no canvas, no 256 squares: the night is gone
    expect(document.querySelector('canvas, .error-night, [data-tube]')).toBeNull();
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

    // the index was asked, so the verse says so; the finding names the alternative
    const h1 = screen.getByRole('heading', {
      level: 1,
      name: "no transaction at this address. the local index was asked; it holds none. honey badger don't care.",
    });
    expect(verseLines(h1)[0]).toBe('no transaction at this address.');
    expect(screen.queryByText(/the chain was not asked/)).toBeNull();
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

  it('preserves and wraps a full endpoint in a verified finding; the seal is the tip’s, not the asked hash', async () => {
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
    // nothing answered: the same verse as an entity known as something else
    expect(verseLines(screen.getByRole('heading', { level: 1 }))).toEqual([
      'no transaction at this address.',
      'the local index was asked; it holds none.',
      "honey badger don't care.",
    ]);
    // the seal pressed behind is the newest block's, never the hash that was asked for
    const seal = await screen.findByTestId('error-seal');
    expect(seal.querySelector('svg')).toHaveAttribute(
      'aria-label',
      `Hash seal ${TIP_HASH.slice(0, 10)}`
    );
    expect(seal.querySelector('svg')).not.toHaveAttribute(
      'aria-label',
      `Hash seal ${HASH.slice(0, 10)}`
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
    // the words of the line wrap between words; only the endpoint and the identifier break anywhere
    const endpoint = screen.getByText('/transactions/0x' + 'f'.repeat(64));
    expect(endpoint).toHaveClass('break-all');
    expect(endpoint.parentElement).not.toHaveClass('break-all');
    expect(endpoint.parentElement).toHaveClass('[overflow-wrap:anywhere]');
  });

  it('an entity’s verse: its kind in words, the index asked, and the meme last — never the route’s second line', () => {
    expect(
      entityVerse({
        kind: 'fiber-channel',
        identifier: '0x01',
        checkedEndpoint: '/fiber/channels/0x01',
      })
    ).toEqual([
      'no fiber channel at this address.',
      'the local index was asked; it holds none.',
      "honey badger don't care.",
    ]);
    expect(entityVerse({ kind: 'block', identifier: '1', checkedEndpoint: '/blocks/1' })[0]).toBe(
      'no block at this address.'
    );
    expect(
      entityVerse({ kind: 'script-family', identifier: 'x', checkedEndpoint: '/x' })
    ).not.toContain(ROUTE_VERSE[1]);
  });
});
