// The cell page's order (lane D3; UI review U-17: "transactions", where the cell came from and
// went, every reader's, stood after the byte map; the page had no developer tier): its
// transactions stand before its bytes, and a developer tier holds the generic out-point paragraph,
// the byte map and the transactions' script groups.
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import CellDetailPage from '@/app/cell/[outpoint]/client-page';
import { api, type Cell } from '@/lib/api';
import sporeCell from '@/__tests__/fixtures/d5/cell-spore-board.json';

const cell = sporeCell as unknown as Cell;
vi.mock('@/src/navigation', () => ({
  useParams: () => ({ outpoint: `${cell.txHash}-1` }),
  usePathname: () => '/cell',
  useRouter: () => ({ push: vi.fn() }),
}));
vi.mock('@/components/cell/cell-transactions', () => ({
  CellTransactions: () => <section data-testid="transactions" />,
  CellScriptGroups: () => <section data-testid="script-groups" />,
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div /> }));
vi.mock('@/lib/api', () => ({
  api: {
    getCell: vi.fn(),
    lookupScripts: vi.fn(),
    getSporeObject: vi.fn(),
    getSporeCluster: vi.fn(),
  },
  isNotFoundApiError: () => false,
}));

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.getCell).mockResolvedValue(cell);
  vi.mocked(api.lookupScripts).mockResolvedValue({});
  // the inventory line's reads, as the Cell board's tests give them (2026-11-03's local API)
  const clusterId = '0x26a1fe3f4e84ae2cd1dc3e319ca416e27624c5ebe2f1b0aa731ca18d67784c4c';
  vi.mocked(api.getSporeObject).mockResolvedValue({
    sporeId: cell.type!.args,
    clusterId,
    mediaProfile: { tier: 'btc_ckb' },
  } as never);
  vi.mocked(api.getSporeCluster).mockResolvedValue({ clusterId, name: 'Nervape 3x' } as never);
});

function view() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <CellDetailPage />
    </QueryClientProvider>
  );
}

const follows = (a: Node, b: Node) =>
  Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);

describe('the cell page: its transactions before its bytes, a developer tier (lane D3)', () => {
  it('stands the transactions in tier 1 and the bytes, the out-point note and the groups in tier 2', async () => {
    const { container } = view();
    const transactions = await screen.findByTestId('transactions');
    const bytes = await screen.findByRole('heading', { name: /^Bytes / });
    const tier1 = container.querySelector<HTMLElement>('[data-tier="1"]')!;
    const tier2 = container.querySelector<HTMLElement>('[data-tier="2"]')!;
    expect(tier2).not.toBeNull();
    expect(tier1).toContainElement(transactions);
    expect(follows(transactions, bytes)).toBe(true);
    const note = screen.getByText(/The out point is the cell's only name/);
    const groups = screen.getByTestId('script-groups');
    for (const el of [note, bytes, groups]) {
      expect(tier2).toContainElement(el);
      expect(tier1).not.toContainElement(el);
    }
    // in tier 2: the note on the cell's name, its bytes, then its transactions' groups
    expect(follows(note, bytes)).toBe(true);
    expect(follows(bytes, groups)).toBe(true);
    // the lede keeps what the cell is, not the generic note. Re-pinned on purpose (lane P1, said
    // once): it is one sentence of what the cell is; the origin line it held went
    const lede = container.querySelector<HTMLElement>('[data-detail-lede]')!;
    expect(screen.queryByTestId('cell-origin')).toBeNull();
    expect(lede.textContent).toMatch(/^This cell /);
    expect(lede).not.toContainElement(note);
  });
});
