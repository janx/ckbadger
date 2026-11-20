import { describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import { createMemoryRouter, RouterProvider } from 'react-router-dom';
import { render } from '@/__tests__/utils/test-utils';
import TokensPage from '@/app/inventory/tokens/page';
import ObjectsPage from '@/app/inventory/objects/page';
import IdentitiesPage from '@/app/inventory/identities/page';
import { api, type InventorySummary } from '@/lib/api';
import { pagedRead } from '@/__tests__/utils/paged-read';

/**
 * The inventory's three classes are three addresses (`/inventory/tokens`, `/objects`,
 * `/identities`): choosing a class goes to its address as a step of history, so the browser's back
 * walks it, under the network of the address it leaves. Written by the controls lane (C2, c2-lists)
 * before the class row became the controls board's tab row, so the new row is proven against it.
 */
vi.mock('@/src/navigation', async (importOriginal) => await importOriginal());
vi.mock('@/lib/api', () => ({
  api: { getAssets: vi.fn(), getInventorySummary: vi.fn() },
}));
vi.mock('@/components/layout/header', () => ({ Header: () => null }));

const summary: InventorySummary = {
  source: 'canonical_asset_indexes',
  anchor: null,
  counts: { token: 727, object: 5653, identity: 4 },
  objectStandards: [],
  objectComposition: [{ tier: 'pure_ckb', collectionsCount: 5653, liveCount: 100 }],
  fullyOnchainObjectsCount: 100,
  tokenStandards: [
    { standard: 'sudt', count: 94 },
    { standard: 'xudt', count: 629 },
    { standard: 'xudt_compatible', count: 4 },
  ],
  pendingItemsCount: 0,
};

function renderAt(url: string) {
  vi.mocked(api.getInventorySummary).mockResolvedValue(summary);
  vi.mocked(api.getAssets).mockResolvedValue(
    pagedRead({
      data: [],
      total: 0,
      limit: 50,
      hasMore: false,
      nextCursor: null,
      anchor: null,
    })
  );
  const router = createMemoryRouter(
    [
      { path: '/:network/inventory/tokens', element: <TokensPage /> },
      { path: '/:network/inventory/objects', element: <ObjectsPage /> },
      { path: '/:network/inventory/identities', element: <IdentitiesPage /> },
    ],
    { initialEntries: [url] }
  );
  render(<RouterProvider router={router} />);
  return router;
}

describe('the inventory’s classes (behaviour the controls lane keeps)', () => {
  it('goes to a class’s address as a step of history, under the network it leaves', async () => {
    const router = renderAt('/mainnet/inventory/tokens');
    await waitFor(() => expect(api.getAssets).toHaveBeenCalled());
    // Re-pinned on purpose (the same commit as the change, controls lane C2): the class row is
    // the controls board's tab row, a tab per class (it was a link per class). Re-pinned again
    // (lane c2-finish, item 3): a class is its own address, so the row is the tab row's links,
    // each with its href, which opens in a new tab too.
    const objects = await screen.findByRole('link', { name: /^objects/i });
    expect(objects).toHaveAttribute('href', '/mainnet/inventory/objects');
    fireEvent.click(objects);
    await waitFor(() => expect(router.state.location.pathname).toBe('/mainnet/inventory/objects'));
    expect(router.state.historyAction).toBe('PUSH');
    await waitFor(() =>
      expect(api.getAssets).toHaveBeenLastCalledWith(expect.objectContaining({ type: 'object' }))
    );
    fireEvent.click(await screen.findByRole('link', { name: /^identities/i }));
    await waitFor(() =>
      expect(router.state.location.pathname).toBe('/mainnet/inventory/identities')
    );
  });
});
