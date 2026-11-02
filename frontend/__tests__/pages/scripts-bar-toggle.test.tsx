import { describe, it, expect, vi, beforeEach } from 'vitest';
import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { createMemoryRouter, RouterProvider } from 'react-router-dom';
import { render } from '../utils/test-utils';
import ScriptsPage from '@/app/scripts/page';
import { api, type KnownScript } from '@/lib/api';

// The real navigation (setup.ts stubs it with an empty URL), so a router's location reaches the page.
vi.mock('@/src/navigation', async (importOriginal) => await importOriginal());
vi.mock('@/lib/api', () => ({
  api: { getScripts: vi.fn() },
  isWarmupPendingError: () => false,
  isNetworkInitializingError: vi.fn(() => false),
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div /> }));

// Table-rows plan L1-2 (ruling D2): the list's bar shows the sort column's CKB by default; a
// two-segment control beside "sorted by" switches it to the occupied share. The choice lives in the
// URL (`?bar=occupied`), read on load, written by replace, followed when the URL changes (the
// pattern D11 gave /transactions?kind=).
function family(name: string, capacity: string, occupied: string): KnownScript {
  return {
    codeHash: `family:${name}`,
    name,
    description: null,
    scriptKind: 'lock',
    rfc: null,
    website: null,
    sourceUrl: null,
    decoderType: null,
    network: '',
    hashType: null,
    dataHash: null,
    typeHash: null,
    tag: null,
    deprecated: false,
    isSystem: false,
    codeCellTxHash: null,
    codeCellOutputIndex: null,
    deployedAt: null,
    ownedCapacitySum: capacity,
    ownedKnowledgeSum: occupied,
    liveCellsCount: 1,
    cellsCount: 1,
    versionsCount: 1,
    codeClasses: [],
    firstDeployedAt: null,
  };
}

// 40B CKB, 0.14 % occupied; 4B CKB, 50 % occupied; 0 CKB.
const big = family('Big', '4000000000000000000', '5600000000000000');
const half = family('Half', '400000000000000000', '200000000000000000');
const none = family('None', '0', '0');
const response = {
  data: [big, half, none],
  total: 3,
  limit: 50,
  hasMore: false,
  nextCursor: null,
  roleCounts: { all: 3, locks: 3, types: 0 },
};

function renderAt(url: string) {
  const router = createMemoryRouter([{ path: '*', element: <ScriptsPage /> }], {
    initialEntries: [url],
  });
  render(<RouterProvider router={router} />);
  return router;
}
const control = () => screen.getByRole('group', { name: 'Bar shows' });
const option = (name: string) => within(control()).getByRole('button', { name });
const fill = async (name: string) => {
  const table = await screen.findByRole('table', { name: 'Scripts' });
  const row = within(table).getByRole('link', { name }).closest('tr') as HTMLElement;
  const track = row.querySelector<HTMLElement>('[data-track]')!;
  const segment = row.querySelector<HTMLElement>('[data-segment]')!;
  return Number.parseFloat(segment.style.width) / Number.parseFloat(track.style.width);
};
const ticks = async () => {
  const table = await screen.findByRole('table', { name: 'Scripts' });
  const head = within(table).getAllByRole('columnheader')[3];
  return [...head.querySelectorAll('[data-scale-ticks] > span')].map((n) => n.textContent);
};

describe('Scripts bar: CKB or occupied share (table-rows L1-2, D2)', () => {
  beforeEach(() => {
    vi.mocked(api.getScripts).mockResolvedValue(response);
  });

  it('sits right of "sorted by" and shows CKB by default, on the page scale', async () => {
    renderAt('/mainnet/scripts');
    await screen.findByRole('table', { name: 'Scripts' });
    const sort = screen.getByRole('combobox', { name: 'Sort scripts' });
    expect(sort.compareDocumentPosition(control()) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(control()).toHaveTextContent(/^bar:\s*CKB\s*occupied$/);
    expect(option('CKB')).toHaveAttribute('aria-pressed', 'true');
    expect(option('occupied')).toHaveAttribute('aria-pressed', 'false');
    expect(await ticks()).toEqual(['0', '20B', '40B']);
    expect(await fill('Big')).toBeCloseTo(1, 3);
    expect(await fill('Half')).toBeCloseTo(0.1, 3);
    expect(await fill('None')).toBe(0);
  });

  it('starts on the occupied share when the URL says ?bar=occupied: occupied ÷ CKB, 0 · 50 · 100 %', async () => {
    renderAt('/mainnet/scripts?bar=occupied');
    expect(await ticks()).toEqual(['0', '50', '100 %']);
    expect(option('occupied')).toHaveAttribute('aria-pressed', 'true');
    const table = await screen.findByRole('table', { name: 'Scripts' });
    expect(within(table).getAllByRole('columnheader')[3]).toHaveTextContent(
      'CKB in its cells · bar: occupied share'
    );
    expect(await fill('Half')).toBeCloseTo(0.5, 3);
    // 0.14 % is under 2 px of the track: still a 2 px mark (T4); zero stays empty.
    const row = within(table).getByRole('link', { name: 'Big' }).closest('tr') as HTMLElement;
    expect(Number.parseFloat(row.querySelector<HTMLElement>('[data-segment]')!.style.width)).toBe(
      2
    );
    expect(await fill('None')).toBe(0);
    // The printed amount is still the CKB in the cells.
    expect(row.querySelector('[data-ckb-2dp]')).toHaveTextContent('40,000,000,000.00');
  });

  it('writes the choice to the URL by replace, and CKB drops it', async () => {
    const router = renderAt('/mainnet/scripts');
    await screen.findByRole('table', { name: 'Scripts' });
    fireEvent.click(option('occupied'));
    await waitFor(() => expect(router.state.location.search).toBe('?bar=occupied'));
    expect(router.state.location.pathname).toBe('/mainnet/scripts');
    expect(router.state.historyAction).toBe('REPLACE');
    expect(await ticks()).toEqual(['0', '50', '100 %']);
    fireEvent.click(option('CKB'));
    await waitFor(() => expect(router.state.location.search).toBe(''));
    expect(await ticks()).toEqual(['0', '20B', '40B']);
  });

  it('follows the URL when it changes under the mounted page, and ignores an unknown value', async () => {
    const router = renderAt('/mainnet/scripts?bar=occupied');
    expect(await ticks()).toEqual(['0', '50', '100 %']);
    await act(async () => {
      await router.navigate('/mainnet/scripts?bar=ckb');
    });
    expect(option('CKB')).toHaveAttribute('aria-pressed', 'true');
    expect(await ticks()).toEqual(['0', '20B', '40B']);
    await act(async () => {
      await router.navigate('/mainnet/scripts?bar=occupied');
    });
    expect(option('occupied')).toHaveAttribute('aria-pressed', 'true');
    await act(async () => {
      await router.navigate('/mainnet/scripts?bar=nonsense');
    });
    expect(option('CKB')).toHaveAttribute('aria-pressed', 'true');
  });

  it('keeps the sort and the table request when the bar changes (a view choice, not a query)', async () => {
    renderAt('/mainnet/scripts');
    await screen.findByRole('table', { name: 'Scripts' });
    const calls = vi.mocked(api.getScripts).mock.calls.length;
    fireEvent.click(option('occupied'));
    expect(await ticks()).toEqual(['0', '50', '100 %']);
    expect(vi.mocked(api.getScripts).mock.calls.length).toBe(calls);
    expect(screen.getByRole('combobox', { name: 'Sort scripts' })).toHaveValue('capacity');
  });
});
