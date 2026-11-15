import { MemoryRouter } from 'react-router-dom';
import { describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor, within } from '@/__tests__/utils/test-utils';
import { entityVerse, NotFoundPage, notAnIdVerse, ROUTE_VERSE } from '@/components/not-found-page';
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

/** The pressed seal once its drawing (a lazy chunk) has arrived. */
async function pressedSeal(): Promise<HTMLElement> {
  const seal = await screen.findByTestId('error-seal');
  await waitFor(() => expect(seal.querySelector('[data-seal] svg')).not.toBeNull());
  return seal;
}

/** What the tip block's stone says (MSW's `/blocks/:id`: TIP_HASH, 5 transactions). */
const TIP_SEAL_LABEL = `Block seal ${TIP_HASH.slice(0, 10)}, 5 transactions`;

/** The facts' lines under the verse, the ways on (a nav) aside. */
function factLines(): HTMLElement[] {
  const facts = document.querySelector('.error-facts');
  if (!(facts instanceof HTMLElement)) throw new Error('the page has no facts');
  return Array.from(facts.children).filter(
    (el): el is HTMLElement => el instanceof HTMLElement && el.tagName !== 'NAV'
  );
}

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
    const seal = await pressedSeal();
    expect(seal.querySelectorAll('[data-seal]')).toHaveLength(1);
    expect(seal.querySelector('[data-seal="block"][data-seal-form="page"] svg')).toHaveAttribute(
      'aria-label',
      TIP_SEAL_LABEL
    );
    expect(document.querySelector('[data-hash-seal]')).toBeNull();
    // no WebGL, no canvas, no 256 squares: the night is gone
    expect(document.querySelector('canvas, .error-night, [data-tube]')).toBeNull();
  });

  it('an entity known as something else: the hash is a fact, and the verified alternative is the way on', () => {
    vi.mocked(useNotFoundEvidence).mockReturnValueOnce({
      status: 'checked',
      evidence: {
        checkedEndpoint: '/transactions/0xabc',
        query: 'block:0xabc',
        searchEndpoint: '/search',
        finding: '/search: this hash is a block',
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
      name: "no transaction with this hash. the local index was asked; it holds none. honey badger don't care.",
    });
    expect(verseLines(h1)[0]).toBe('no transaction with this hash.');
    expect(screen.queryByText(/the chain was not asked/)).toBeNull();
    expect(screen.getByText('/transactions/0xabc')).toBeInTheDocument();
    // U-18: what was asked, its answer and what /search said, in one line
    expect(factLines()[0]).toHaveTextContent(
      /^checked \/transactions\/0xabc → 404 · \/search: this hash is a block → open block #42$/
    );
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

  // U-18: the diagnostics were two lines saying the same thing, the full 66-character hash three
  // times, and `break-all` on the whole finding cut "r / eturned" on a phone.
  it('says what was asked in one line, the id once and shortened; the seal is the tip’s, not the asked hash', async () => {
    const endpoint = `/transactions/${HASH}/detail`;
    vi.mocked(useNotFoundEvidence).mockReturnValueOnce({
      status: 'checked',
      evidence: {
        checkedEndpoint: endpoint,
        query: `block:${HASH}`,
        searchEndpoint: '/search',
        finding: '/search: no block has this hash either',
      },
    });

    render(
      <MemoryRouter initialEntries={[`/mainnet/tx/${HASH}`]}>
        <NotFoundPage
          entity={{ kind: 'transaction', identifier: HASH, checkedEndpoint: endpoint }}
        />
      </MemoryRouter>
    );

    // U-09 (re-pinned): the endpoint's id at the row size, 10…6
    const short = `${HASH.slice(0, 10)}…${HASH.slice(-6)}`;
    const lines = factLines();
    expect(lines).toHaveLength(1);
    expect(lines[0]).toHaveTextContent(
      `checked /transactions/${short}/detail → 404 · /search: no block has this hash either`
    );
    expect(document.querySelector('.error-facts')).not.toHaveTextContent(HASH);
    // the words of the line wrap between words; the shortened id is one unit (U-16, re-pinned:
    // it broke anywhere, "0x" / "36528a03...742ce54b")
    const id = within(lines[0]).getByText(short);
    expect(id).toHaveAttribute('data-nobreak');
    expect(lines[0].querySelectorAll('.break-all')).toHaveLength(0);
    // nothing answered: the same verse as an entity known as something else
    expect(verseLines(screen.getByRole('heading', { level: 1 }))).toEqual([
      'no transaction with this hash.',
      'the local index was asked; it holds none.',
      "honey badger don't care.",
    ]);
    // the seal pressed behind is the newest block's, never the hash that was asked for
    const seal = await pressedSeal();
    expect(seal.querySelector('svg')).toHaveAttribute('aria-label', TIP_SEAL_LABEL);
    expect(seal.querySelector('svg')?.getAttribute('aria-label')).not.toContain(HASH.slice(0, 10));
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
      'the /search check could not complete: search unavailable'
    );
    // the same one line: the check, its 404, and that /search could not say more
    const lines = factLines();
    expect(lines).toHaveLength(1);
    expect(lines[0]).toHaveTextContent(
      'checked /transactions/0xffffffff…ffffff → 404 · the /search check could not complete: search unavailable'
    );
    // the words of the line wrap between words; only the id breaks anywhere
    expect(within(lines[0]).getByText('0xffffffff…ffffff')).toHaveAttribute('data-nobreak');
    expect(lines[0]).not.toHaveClass('break-all');
    expect(lines[0]).toHaveClass('[overflow-wrap:anywhere]');
  });

  it('a short id is shown as it is, and an endpoint with no exact /search says only its check', () => {
    vi.mocked(useNotFoundEvidence).mockReturnValueOnce({
      status: 'checked',
      evidence: {
        checkedEndpoint: '/forks/7/a9bc1b75',
        query: null,
        searchEndpoint: null,
        finding: null,
      },
    });
    render(
      <NotFoundPage
        entity={{ kind: 'fork', identifier: '7/a9bc1b75', checkedEndpoint: '/forks/7/a9bc1b75' }}
      />
    );
    const lines = factLines();
    expect(lines).toHaveLength(1);
    expect(lines[0]).toHaveTextContent(/^checked \/forks\/7\/a9bc1b75 → 404$/);
    expect(lines[0].querySelector('.break-all')).toBeNull();
  });

  // U-06, R11: a malformed id gets the same page, with its own first line and the reason.
  it('an id that is not one: its kind’s first line, the index not asked, the reason as one quiet line', async () => {
    render(
      <MemoryRouter initialEntries={['/mainnet/address/ckb1notanaddress']}>
        <NotFoundPage notAnId={{ kind: 'address', reason: 'Invalid CKB address: parse failed' }} />
      </MemoryRouter>
    );
    const h1 = screen.getByRole('heading', { level: 1 });
    expect(verseLines(h1)).toEqual([
      'that is not a ckb address.',
      'the index was not asked; nothing on it is lost.',
      "honey badger don't care.",
    ]);
    expect(h1).toHaveClass('comment', 'comment-lg');
    expect(screen.getByText('Not an id · 400')).toHaveClass('eyebrow');
    expect(screen.queryByText('Not found · 404')).toBeNull();
    const lines = factLines();
    expect(lines).toHaveLength(1);
    expect(lines[0]).toHaveTextContent(/^Invalid CKB address: parse failed$/);
    expect(useNotFoundEvidence).toHaveBeenLastCalledWith(undefined);
    expect(screen.getByRole('link', { name: '← activities' })).toHaveAttribute(
      'href',
      '/mainnet/activities'
    );
    await waitFor(() => expect(document.title).toBe('Not found · CKBadger'));
  });

  it('every kind of id has its own first line', () => {
    expect(notAnIdVerse('block')[0]).toBe('that is not a block number or hash.');
    expect(notAnIdVerse('transaction')[0]).toBe('that is not a transaction hash.');
    expect(notAnIdVerse('cell')[0]).toBe('that is not a cell out point.');
    expect(notAnIdVerse('object')[0]).toBe('that is not an object id.');
    expect(notAnIdVerse('collection')[0]).toBe('that is not a collection id.');
    expect(notAnIdVerse('fiber-channel')[0]).toBe('that is not a channel id.');
    expect(notAnIdVerse('token')[0]).toBe('that is not a token type hash.');
    expect(notAnIdVerse('script')[0]).toBe('that is not a script code hash.');
    expect(notAnIdVerse('identity')[0]).toBe('that is not an identity id.');
    expect(notAnIdVerse('fork')[0]).toBe('that is not a fork height and orphan.');
    expect(notAnIdVerse('uncle')[0]).toBe('that is not a block and uncle index.');
    expect(notAnIdVerse('uncle').slice(1)).toEqual([
      'the index was not asked; nothing on it is lost.',
      "honey badger don't care.",
    ]);
  });

  // U-15 (UI review 2026-11-14): "no transaction at this address." — a transaction has a hash, not
  // an address. Each kind's first line names its id as it is; an address keeps "at this address".
  it('names each kind’s id honestly in the first line, an address alone "at this address"', () => {
    const first = (kind: Parameters<typeof notAnIdVerse>[0]) =>
      entityVerse({ kind, identifier: 'x', checkedEndpoint: '/x' })[0];
    expect(first('block')).toBe('no block with this number or hash.');
    expect(first('uncle')).toBe('no uncle at this block and index.');
    expect(first('transaction')).toBe('no transaction with this hash.');
    expect(first('cell')).toBe('no cell at this out point.');
    expect(first('address')).toBe('nothing at this address.');
    expect(first('script')).toBe('no script with this code hash.');
    expect(first('script-family')).toBe('no script with this name.');
    expect(first('token')).toBe('no token with this type hash.');
    expect(first('object')).toBe('no object with this id.');
    expect(first('collection')).toBe('no collection with this id.');
    expect(first('identity')).toBe('no identity with this id.');
    expect(first('class')).toBe('no class with this id.');
    expect(first('cluster')).toBe('no cluster with this id.');
    expect(first('fork')).toBe('no fork at this height and orphan.');
    expect(first('fiber-channel')).toBe('no fiber channel with this id.');
  });

  it('an entity’s verse: its kind in words, the index asked, and the meme last — never the route’s second line', () => {
    expect(
      entityVerse({
        kind: 'fiber-channel',
        identifier: '0x01',
        checkedEndpoint: '/fiber/channels/0x01',
      })
    ).toEqual([
      'no fiber channel with this id.',
      'the local index was asked; it holds none.',
      "honey badger don't care.",
    ]);
    expect(entityVerse({ kind: 'block', identifier: '1', checkedEndpoint: '/blocks/1' })[0]).toBe(
      'no block with this number or hash.'
    );
    expect(
      entityVerse({ kind: 'script-family', identifier: 'x', checkedEndpoint: '/x' })
    ).not.toContain(ROUTE_VERSE[1]);
  });
});
