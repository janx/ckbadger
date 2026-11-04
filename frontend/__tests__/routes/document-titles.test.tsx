// Consistency plan C1 (review finding A): every routed page names itself in the browser tab — no
// page keeps index.html's slogan except home. The route table is read from `createAppRouter()`,
// so a new route without a title fails here until it is listed with its sample address and title.
import { createMemoryRouter, RouterProvider, type RouteObject } from 'react-router-dom';
import { http, HttpResponse } from 'msw';
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { server } from '@/__tests__/msw/server';
import { render, waitFor } from '@/__tests__/utils/test-utils';
import { createAppRouter } from '@/src/routes/router';

vi.mock('@/src/navigation', async (importOriginal) => importOriginal());
vi.mock('@/components/layout/header', () => ({ Header: () => null }));
vi.mock('@/components/layout/site-footer', () => ({ SiteFooter: () => null }));
vi.mock('@/components/error-vessels', () => ({
  ErrorVessels: () => null,
  VesselsCaption: () => null,
}));

const SLOGAN = 'CKBadger — Local-first CKB-native Explorer';
const HASH = '0xdcd14a81fb8897d10280153792b4fe99ea9dc991c7d8ca5ac7700f72e91a334d';

/** One sample address per route pattern (the review's page list where it has one). */
const SAMPLE: Record<string, string> = {
  'blocks/:id': 'blocks/20894843',
  'blocks/:id/uncles/:index': 'blocks/20894834/uncles/0',
  'tx/:hash': `tx/${HASH}`,
  'address/:addr':
    'address/ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqfs6k9ygrwtdrn828l4a04ns9fnes960jg3x8k26',
  'cell/:outpoint': `cell/${HASH}-0`,
  'forks/:height/:orphan': 'forks/20894012/c121e093',
  'fiber/channels/:id': `fiber/channels/${HASH}`,
  'script/:codeHash': `script/${HASH}`,
  'scripts/:name': 'scripts/Default%20Lock',
  'tokens/:typeHash': `tokens/${HASH}`,
  'clusters/:clusterId': `clusters/${HASH}`,
  'identities/:collectionId': 'identities/dotbit',
  'classes/:classId': 'classes/0x8f67efedd50c61c9dd332defd4051f08a02d797700000014',
  'objects/:sporeId': `objects/${HASH}`,
  'objects/mnft/:objectId':
    'objects/mnft/0x8f67efedd50c61c9dd332defd4051f08a02d79770000001400000000',
  'identities/dotbit/:identityId': 'identities/dotbit/0x0000006c18b43833ffdfbdd5aba20c28c9affdc6',
  'identities/did/:identityId': `identities/did/${HASH}`,
  'identities/bit-cell/:identityId': `identities/bit-cell/${HASH}`,
  'identities/dotcell/:identityId': 'identities/dotcell/verification.cell',
  '*': 'nope-404',
};

function routePaths(): string[] {
  const network = createAppRouter().find((r) => r.path === '/:network') as RouteObject;
  // the page routes sit under one pathless route that holds their error boundary (visual finish V6)
  const pages = (network.children ?? []).flatMap((r) =>
    r.path === undefined && !r.index ? (r.children ?? []) : [r]
  );
  return pages.map((r) => (r.index ? '' : (r.path as string)));
}

describe('every route names its page in the tab (C1)', () => {
  beforeAll(() => {
    window.__CKBADGER_RUNTIME_CONFIG__ = {
      networks: [{ name: 'mainnet' }],
      defaultNetwork: 'mainnet',
    };
  });
  afterAll(() => {
    delete window.__CKBADGER_RUNTIME_CONFIG__;
  });
  beforeEach(() => {
    document.title = SLOGAN;
    vi.spyOn(console, 'error').mockImplementation(() => {});
    vi.spyOn(console, 'warn').mockImplementation(() => {});
    server.use(
      http.all('*', () =>
        HttpResponse.json({ error: 'not_found', message: 'not in this test' }, { status: 404 })
      )
    );
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  function mount(path: string) {
    const router = createMemoryRouter(createAppRouter(), { initialEntries: [`/mainnet/${path}`] });
    return render(<RouterProvider router={router} />);
  }

  // Every API answer here is a 404, so a page is titled in whichever state it reaches first —
  // loading, its own header, or the not-found page; the named titles with data are asserted in
  // the page tests and on the live survey (gate §3).
  it.each(
    routePaths()
      .filter((p) => p !== '')
      .map((p) => [p])
  )('/%s', async (pattern) => {
    const path = SAMPLE[pattern] ?? pattern;
    expect(path, `route ${pattern} needs a sample address in SAMPLE`).not.toMatch(/:/);
    const view = mount(path);
    // Each page is a lazy chunk; under the full suite it can take over a second to load.
    await waitFor(() => expect(document.title).not.toBe(SLOGAN), { timeout: 5000 });
    expect(document.title).toMatch(/^\S.* · CKBadger$/);
    view.unmount();
    expect(document.title).toBe(SLOGAN);
  });

  it('home keeps the slogan from index.html', async () => {
    const view = mount('');
    await waitFor(() => expect(document.querySelector('main')).not.toBeNull(), { timeout: 5000 });
    expect(document.title).toBe(SLOGAN);
    view.unmount();
  });

  it('a route that names no page is "Not found · CKBadger"', async () => {
    mount('nope-404');
    await waitFor(() => expect(document.title).toBe('Not found · CKBadger'), { timeout: 5000 });
  });
});
