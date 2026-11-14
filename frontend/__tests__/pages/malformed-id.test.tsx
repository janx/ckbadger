import type { ReactElement, ReactNode } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { QueryClientProvider } from '@tanstack/react-query';
import { http, HttpResponse, type PathParams } from 'msw';
import { render, screen, waitFor, within } from '@testing-library/react';
import { server } from '@/__tests__/msw/server';
import { createAppQueryClient } from '@/lib/query-client';
import AddressDetailPage from '@/app/address/[addr]/client-page';
import BlockDetailPage from '@/app/blocks/[id]/client-page';
import UncleDetailPage from '@/app/blocks/[id]/uncles/[index]/client-page';
import TransactionDetailPage from '@/app/tx/[hash]/client-page';
import CellDetailPage from '@/app/cell/[outpoint]/client-page';
import SporeDetailPage from '@/app/objects/[sporeId]/client-page';
import MnftItemDetailPage from '@/app/objects/mnft/[objectId]/client-page';
import FiberChannelDetailPage from '@/app/fiber/channels/[id]/client-page';
import ClusterDetailPage from '@/app/clusters/[clusterId]/client-page';
import MnftClassDetailPage from '@/app/classes/[classId]/client-page';
import IdentityCollectionPage from '@/app/identities/[collectionId]/client-page';
import DotbitItemDetailPage from '@/app/identities/dotbit/[identityId]/client-page';
import DidCkbItemDetailPage from '@/app/identities/did/[identityId]/client-page';
import BitCellItemDetailPage from '@/app/identities/bit-cell/[identityId]/client-page';
import DotCellItemDetailPage from '@/app/identities/dotcell/[identityId]/client-page';
import TokenDetailPage from '@/app/tokens/[typeHash]/client-page';
import ForkDetailPage from '@/app/forks/[height]/[orphan]/client-page';
import ScriptByCodeHashPage from '@/app/script/[codeHash]/client-page';

// U-06, PA-01, R11: a malformed id gets the same voice page as a missing one, with its own first
// line and the reason under the verse; the API's 400 is a definite answer, asked once.

const nav = vi.hoisted(() => ({
  params: {} as Record<string, string>,
  pathname: '/',
  replace: vi.fn(),
}));
vi.mock('@/src/navigation', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/src/navigation')>()),
  useParams: () => nav.params,
  usePathname: () => nav.pathname,
  useRouter: () => ({ replace: nav.replace, push: vi.fn(), back: vi.fn(), prefetch: vi.fn() }),
}));

const SECOND = 'the index was not asked; nothing on it is lost.';
const THIRD = "honey badger don't care.";

/** The app's own query client, retries and all, so a retried 400 would show in the count. */
function renderWithAppClient(ui: ReactElement) {
  const client = createAppQueryClient({ gcTime: Infinity });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  return render(ui, { wrapper });
}

/**
 * The endpoint answers 400 for `matches`, counted; every other request goes on to the default
 * handlers. The id's sub-resources (`also`: the endpoint's own and other families' paths under
 * the same id) answer 400 as the API does, uncounted: the page's other reads of a malformed id.
 */
function answer400(
  path: string,
  matches: (params: PathParams) => boolean,
  message: string,
  also: readonly string[] = []
) {
  const seen = { count: 0 };
  const body = () => HttpResponse.json({ error: 'bad_request', message }, { status: 400 });
  server.use(
    http.get(`/api/:network/v1${path}`, ({ params }) => {
      if (!matches(params)) return undefined;
      seen.count += 1;
      return body();
    }),
    ...[path, ...also].map((under) =>
      http.get(`/api/:network/v1${under}/*`, ({ params }) => (matches(params) ? body() : undefined))
    )
  );
  return seen;
}

function at(pathname: string, params: Record<string, string>) {
  nav.pathname = pathname;
  nav.params = params;
}

async function expectNotAnId(firstLine: string, reason: string) {
  const h1 = await screen.findByRole('heading', {
    level: 1,
    name: `${firstLine} ${SECOND} ${THIRD}`,
  });
  expect(Array.from(h1.querySelectorAll('[data-comment-line]'), (l) => l.textContent)).toEqual([
    firstLine,
    SECOND,
    THIRD,
  ]);
  expect(screen.getByText('Not an id · 400')).toHaveClass('eyebrow');
  const facts = document.querySelector('.error-facts');
  if (!(facts instanceof HTMLElement)) throw new Error('the voice page has no facts');
  // the reason is one quiet line; nothing was looked up, so nothing claims a check
  expect(within(facts).getByText(reason)).toBeInTheDocument();
  expect(facts).not.toHaveTextContent(/checked|→ 404/);
  expect(screen.queryByRole('status', { name: 'Loading …' })).toBeNull();
  await waitFor(() => expect(document.title).toBe('Not found · CKBadger'));
}

afterEach(() => {
  nav.replace.mockReset();
});

describe('a malformed id answered 400 by the API', () => {
  it('/address/ckb1notanaddress: not a ckb address, asked once and not polled', async () => {
    at('/address/ckb1notanaddress', { addr: 'ckb1notanaddress' });
    const seen = answer400(
      '/addresses/:addr',
      (p) => p.addr === 'ckb1notanaddress',
      'Invalid CKB address: parse failed'
    );
    renderWithAppClient(<AddressDetailPage />);
    await expectNotAnId('that is not a ckb address.', 'Invalid CKB address: parse failed');
    expect(seen.count).toBe(1);
  });

  it('/blocks/abc: not a block number or hash', async () => {
    at('/blocks/abc', { id: 'abc' });
    const message = 'Invalid block id: expected a block number or a 32-byte hash';
    const seen = answer400('/blocks/:id', (p) => p.id === 'abc', message);
    renderWithAppClient(<BlockDetailPage />);
    await expectNotAnId('that is not a block number or hash.', message);
    expect(seen.count).toBe(1);
  });

  it('/tx/0x1234: not a transaction hash', async () => {
    at('/tx/0x1234', { hash: '0x1234' });
    const message =
      'Invalid transaction hash: expected 32 bytes (64 hex characters, optional 0x prefix), got 2 bytes';
    const seen = answer400('/transactions/:hash/detail', (p) => p.hash === '0x1234', message);
    renderWithAppClient(<TransactionDetailPage />);
    await expectNotAnId('that is not a transaction hash.', message);
    expect(seen.count).toBe(1);
  });

  it('/fiber/channels/0x00: not a channel id', async () => {
    at('/fiber/channels/0x00', { id: '0x00' });
    const message = 'Invalid channel id: expected 32 bytes, got 1 bytes';
    const seen = answer400('/fiber/channels/:id', (p) => p.id === '0x00', message);
    renderWithAppClient(<FiberChannelDetailPage />);
    await expectNotAnId('that is not a channel id.', message);
    expect(seen.count).toBe(1);
  });

  it.each([
    ['/clusters/0x00', (id: string) => <ClusterDetailPage clusterId={id} />],
    ['/classes/0x00', (id: string) => <MnftClassDetailPage classId={id} />],
  ])('%s: not a collection id', async (path, page) => {
    at(path, {});
    const message =
      'Invalid object collection ID (expected a 32-byte cluster or identity collection ID, or a 24-byte m-NFT class ID): expected 32 bytes (64 hex characters, optional 0x prefix), got 1 bytes';
    const seen = answer400('/assets/objects/:id', (p) => p.id === '0x00', message, [
      '/spore/clusters/:id',
    ]);
    renderWithAppClient(page('0x00'));
    await expectNotAnId('that is not a collection id.', message);
    expect(seen.count).toBe(1);
  });

  it('/identities/0x00: not a collection id', async () => {
    at('/identities/0x00', {});
    const message = 'Collection ID is not an identity collection';
    const seen = answer400('/assets/identities/:id', (p) => p.id === '0x00', message);
    renderWithAppClient(<IdentityCollectionPage collectionId="0x00" />);
    await expectNotAnId('that is not a collection id.', message);
    expect(seen.count).toBe(1);
  });

  it.each([
    ['dotbit', (id: string) => <DotbitItemDetailPage identityId={id} />],
    ['did', (id: string) => <DidCkbItemDetailPage identityId={id} />],
    ['bit-cell', (id: string) => <BitCellItemDetailPage identityId={id} />],
    ['dotcell', (id: string) => <DotCellItemDetailPage identityId={id} />],
  ])('/identities/%s/0xzz: not an identity id', async (standard, page) => {
    at(`/identities/${standard}/0xzz`, {});
    const message =
      'Invalid item ID: expected a hex identifier of at most 32 bytes, got invalid hex';
    const seen = answer400(
      `/assets/identities/${standard}/items/:id`,
      (p) => p.id === '0xzz',
      message
    );
    renderWithAppClient(page('0xzz'));
    await expectNotAnId('that is not an identity id.', message);
    expect(seen.count).toBe(1);
  });

  it('/tokens/0x1234: not a token type hash', async () => {
    at('/tokens/0x1234', {});
    const message = 'Invalid type hash: expected 32 bytes, got 2 bytes';
    const seen = answer400('/tokens/:typeHash', (p) => p.typeHash === '0x1234', message);
    renderWithAppClient(<TokenDetailPage typeHash="0x1234" />);
    await expectNotAnId('that is not a token type hash.', message);
    expect(seen.count).toBe(1);
  });

  it('/objects/mnft/0x00: not an object id', async () => {
    at('/objects/mnft/0x00', {});
    const message =
      'Invalid m-NFT token ID: expected 28..=32 bytes of hex (issuer, class and token index), got 1 bytes';
    const seen = answer400('/assets/objects/items/:id', (p) => p.id === '0x00', message);
    renderWithAppClient(<MnftItemDetailPage objectId="0x00" />);
    await expectNotAnId('that is not an object id.', message);
    expect(seen.count).toBe(1);
  });

  it('/blocks/abc/uncles/0: not a block and uncle index', async () => {
    at('/blocks/abc/uncles/0', { id: 'abc', index: '0' });
    const message = 'Invalid block id: expected a block number or a 32-byte hash';
    const seen = answer400('/blocks/:id/uncles/:index', (p) => p.id === 'abc', message);
    renderWithAppClient(<UncleDetailPage />);
    await expectNotAnId('that is not a block and uncle index.', message);
    expect(seen.count).toBe(1);
  });
});

describe('a read that fails without a definite answer', () => {
  /** The endpoint and what is under it answer 500 for `matches`; others go on to the defaults. */
  function answer500(
    path: string,
    matches: (params: PathParams) => boolean,
    also: readonly string[] = []
  ) {
    const body = () =>
      HttpResponse.json(
        { error: 'internal_error', message: 'store read failed at key 0x01' },
        { status: 500 }
      );
    server.use(
      http.get(`/api/:network/v1${path}`, ({ params }) => (matches(params) ? body() : undefined)),
      ...[path, ...also].map((under) =>
        http.get(`/api/:network/v1${under}/*`, ({ params }) =>
          matches(params) ? body() : undefined
        )
      )
    );
  }
  const HASH = `0x${'ab'.repeat(32)}`;

  it.each([
    [
      'block',
      () => {
        at('/blocks/12', { id: '12' });
        answer500('/blocks/:id', (p) => p.id === '12');
        return <BlockDetailPage />;
      },
    ],
    [
      'transaction',
      () => {
        at(`/tx/${HASH}`, { hash: HASH });
        answer500('/transactions/:hash/detail', (p) => p.hash === HASH);
        return <TransactionDetailPage />;
      },
    ],
    [
      'cell',
      () => {
        at(`/cell/${HASH}-0`, { outpoint: `${HASH}-0` });
        answer500('/cells/:hash/:index', (p) => p.hash === HASH);
        return <CellDetailPage />;
      },
    ],
    [
      'channel',
      () => {
        at(`/fiber/channels/${HASH}`, { id: HASH });
        answer500('/fiber/channels/:id', (p) => p.id === HASH);
        return <FiberChannelDetailPage />;
      },
    ],
    [
      'collection',
      () => {
        at(`/clusters/${HASH}`, {});
        answer500('/assets/objects/:id', (p) => p.id === HASH, ['/spore/clusters/:id']);
        return <ClusterDetailPage clusterId={HASH} />;
      },
    ],
    [
      'token',
      () => {
        at(`/tokens/${HASH}`, {});
        answer500('/tokens/:typeHash', (p) => p.typeHash === HASH);
        return <TokenDetailPage typeHash={HASH} />;
      },
    ],
  ])('the %s page says so in the one shared line', async (what, page) => {
    // the test client: no retries, so the failure shows at once
    const client = createAppQueryClient({ gcTime: Infinity, nonWarmupRetry: false });
    render(<QueryClientProvider client={client}>{page()}</QueryClientProvider>);
    const alert = await screen.findByText(
      `Could not read this ${what}: API error: 500 - store read failed at key 0x01`
    );
    expect(alert).toHaveAttribute('role', 'alert');
    expect(document.body).not.toHaveTextContent(/ApiRequestError|Unable to read|Failed to load/);
  });
});

describe('a malformed id the page refuses before asking', () => {
  it('/cell/nonsense: not a cell out point, and nothing is asked', async () => {
    at('/cell/nonsense', { outpoint: 'nonsense' });
    renderWithAppClient(<CellDetailPage />);
    await expectNotAnId(
      'that is not a cell out point.',
      'an out point is a transaction hash and an output index: 0x and 64 hex digits, a dash, 0 to 65535.'
    );
  });

  it('/objects/0x00: not an object id', async () => {
    at('/objects/0x00', {});
    renderWithAppClient(<SporeDetailPage sporeId="0x00" />);
    await expectNotAnId(
      'that is not an object id.',
      'an object id is 0x and 64 hex digits (a Spore or a cluster), or 48 (an m-NFT class).'
    );
  });

  it('/blocks/1/uncles/x: not a block and uncle index', async () => {
    at('/blocks/1/uncles/x', { id: '1', index: 'x' });
    renderWithAppClient(<UncleDetailPage />);
    await expectNotAnId(
      'that is not a block and uncle index.',
      'an uncle is named by its index in the block: a whole number from 0.'
    );
  });

  it('/forks/7/xyz: not a fork height and orphan', async () => {
    at('/forks/7/xyz', { height: '7', orphan: 'xyz' });
    renderWithAppClient(<ForkDetailPage />);
    await expectNotAnId(
      'that is not a fork height and orphan.',
      'a fork is named by its height and the first 8 hex digits of the first orphan’s hash.'
    );
  });

  it('/script/0x1234: not a script code hash, and no redirect to a name', async () => {
    at('/script/0x1234', { codeHash: '0x1234' });
    renderWithAppClient(<ScriptByCodeHashPage codeHash="0x1234" />);
    await expectNotAnId('that is not a script code hash.', 'a code hash is 0x and 64 hex digits.');
    expect(nav.replace).not.toHaveBeenCalled();
  });
});
