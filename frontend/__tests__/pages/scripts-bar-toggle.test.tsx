import { describe, it, expect, vi, beforeEach } from 'vitest';
import { act, screen, within } from '@testing-library/react';
import { createMemoryRouter, RouterProvider } from 'react-router-dom';
import { render } from '../utils/test-utils';
import ScriptsPage from '@/app/scripts/page';
import { api, type KnownScript } from '@/lib/api';
import { pagedRead } from '@/__tests__/utils/paged-read';

// The real navigation (setup.ts stubs it with an empty URL), so a router's location reaches the page.
vi.mock('@/src/navigation', async (importOriginal) => await importOriginal());
vi.mock('@/lib/api', () => ({
  api: { getScripts: vi.fn() },
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div /> }));

// Table-rows plan L1-2 (ruling D2) drew the list's bar as the sort column's CKB, with a switch
// beside "sorted by" (`?bar=occupied` in the URL) to the occupied share. Re-pinned on purpose with
// readability lane B5 (plan §7.6, approved): the CKB bar, a stub on most rows, left the list; the
// occupied share's bar (0–100 %) stays, in the Occupied column beside the share; with one bar left
// the switch has one state and goes, and the page reads no `bar` from its URL.
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
const response = pagedRead({
  data: [big, half, none],
  total: 3,
  limit: 50,
  hasMore: false,
  nextCursor: null,
  roleCounts: { all: 3, locks: 3, types: 0 },
});

function renderAt(url: string) {
  const router = createMemoryRouter([{ path: '*', element: <ScriptsPage /> }], {
    initialEntries: [url],
  });
  render(<RouterProvider router={router} />);
  return router;
}
const fill = async (name: string) => {
  const table = await screen.findByRole('table', { name: 'Scripts' });
  const row = within(table).getByRole('link', { name }).closest('tr') as HTMLElement;
  const track = row.querySelector<HTMLElement>('[data-track]')!;
  const segment = row.querySelector<HTMLElement>('[data-segment]')!;
  return Number.parseFloat(segment.style.width) / Number.parseFloat(track.style.width);
};
/** The ticks of every scale head in the table, by column index. */
const ticks = async () => {
  const table = await screen.findByRole('table', { name: 'Scripts' });
  return within(table)
    .getAllByRole('columnheader')
    .flatMap((head, i) => {
      const scale = head.querySelector('[data-scale-ticks]');
      return scale ? [[i, [...scale.children].map((n) => n.textContent)] as const] : [];
    });
};

describe('Scripts bar: the occupied share alone (readability B5; was the D2 switch)', () => {
  beforeEach(() => {
    vi.mocked(api.getScripts).mockResolvedValue(response);
  });

  // was: 'sits right of "sorted by" and shows CKB by default, on the page scale'
  it('draws no switch and no CKB bar; the one scale is the Occupied column’s 0 · 50 · 100 %', async () => {
    renderAt('/mainnet/scripts');
    await screen.findByRole('table', { name: 'Scripts' });
    expect(screen.queryByRole('group', { name: 'Bar shows' })).toBeNull();
    expect(await ticks()).toEqual([[4, ['0', '50', '100%']]]);
  });

  // was: 'starts on the occupied share when the URL says ?bar=occupied: occupied ÷ CKB, 0 · 50 · 100 %'
  it('draws occupied ÷ CKB in the cells, a 2 px mark under 2 px, and prints the CKB beside', async () => {
    renderAt('/mainnet/scripts');
    const table = await screen.findByRole('table', { name: 'Scripts' });
    expect(within(table).getAllByRole('columnheader')[3]).toHaveTextContent(/^CKB in its cells$/);
    expect(await fill('Half')).toBeCloseTo(0.5, 3);
    // 0.14 % is under 2 px of the track: still a 2 px mark (T4); zero stays empty.
    const row = within(table).getByRole('link', { name: 'Big' }).closest('tr') as HTMLElement;
    expect(Number.parseFloat(row.querySelector<HTMLElement>('[data-segment]')!.style.width)).toBe(
      2
    );
    expect(await fill('None')).toBe(0);
    // The printed amount is the CKB in the cells, in its own column, in the list's summary form
    // (readability R5, re-pinned on purpose: it was the two-place "40,000,000,000.00").
    expect(row.querySelector('[data-ckb-amount]')).toHaveTextContent(/^40,000,000,000$/);
  });

  // was: 'writes the choice to the URL by replace, and CKB drops it' and 'follows the URL when it
  // changes under the mounted page, and ignores an unknown value'
  it('reads no bar from the URL: an old ?bar= link draws the same table and is left as it is', async () => {
    const router = renderAt('/mainnet/scripts?bar=occupied');
    expect(await ticks()).toEqual([[4, ['0', '50', '100%']]]);
    expect(router.state.location.search).toBe('?bar=occupied');
    await act(async () => {
      await router.navigate('/mainnet/scripts?bar=ckb');
    });
    expect(await ticks()).toEqual([[4, ['0', '50', '100%']]]);
    expect(screen.queryByRole('group', { name: 'Bar shows' })).toBeNull();
  });

  // was: 'keeps the sort and the table request when the bar changes (a view choice, not a query)'
  it('asks for the same page whatever ?bar= says (it was a view, never a query)', async () => {
    renderAt('/mainnet/scripts?bar=occupied');
    await screen.findByRole('table', { name: 'Scripts' });
    expect(vi.mocked(api.getScripts).mock.calls.every(([q]) => !('bar' in (q ?? {})))).toBe(true);
    // Re-pinned on purpose (controls lane C2): the controls board's select, named by the label
    // it stands after, "sorted by" (it was the aria-label "Sort scripts").
    expect(screen.getByRole('combobox', { name: 'sorted by' })).toHaveValue('capacity');
  });
});
