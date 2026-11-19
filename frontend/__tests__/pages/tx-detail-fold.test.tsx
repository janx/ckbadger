// The long transaction (lane D3; UI review U-07, PA-21; the transaction board the owner passed at
// gate D0′): a deposit that consumes 158 cells drew 158 tiles, its page 30,316 px at 1280 and its
// deposit 25 screens down on a phone. The page folds each sheet after six tiles; the fold's open
// state is one URL parameter (`cells`), pushed so that back walks it; a cell the URL points at by
// its out point (`#<tx hash>-<index>`, the anchor the cell page links with) is never folded.
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import TransactionDetailPage from '@/app/tx/[hash]/client-page';
import { ErrorBoundary } from '@/components/error-boundary';
import { api } from '@/lib/api';
import {
  LONG_TX_HASH,
  LONG_TX_INPUTS,
  LONG_TX_PREV,
  LONG_TX_SENDER,
  longDeposit,
} from '@/__tests__/utils/long-tx';

let mockSearchParams = new URLSearchParams();
let mockHash = '';
const mockPush = vi.fn();
const mockReplace = vi.fn();

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return {
    ...actual,
    api: {
      getTransactionDetail: vi.fn(),
      getTransactionGraph: vi.fn(),
      getTransactionActivity: vi.fn(),
      getTransactionCellDeps: vi.fn(),
      getTransactionLifecycle: vi.fn(),
      lookupScripts: vi.fn(),
      getBlock: vi.fn(),
    },
  };
});
vi.mock('@/hooks/useCyclesCalculation', () => ({
  useCyclesCalculation: vi.fn(() => ({
    cycles: 5000000,
    hasCycles: true,
    isCalculating: false,
    hasFailed: false,
    isPaused: false,
  })),
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div data-testid="header" /> }));
vi.mock('@/components/cell-graph', () => ({
  default: () => <div data-testid="mock-cell-graph" />,
}));
vi.mock('@/src/navigation', () => ({
  useParams: () => ({ hash: LONG_TX_HASH }),
  usePathname: () => `/tx/${LONG_TX_HASH}`,
  useSearchParams: () => mockSearchParams,
  useSearchAndHash: () => `${mockSearchParams.size ? `?${mockSearchParams}` : ''}${mockHash}`,
  useRouter: () => ({ push: mockPush, replace: mockReplace }),
}));

const SHORT_SENDER = `ckb1…${LONG_TX_SENDER.slice(-8)}`;

beforeEach(() => {
  vi.clearAllMocks();
  mockSearchParams = new URLSearchParams();
  mockHash = '';
  vi.mocked(api.getTransactionDetail).mockResolvedValue(longDeposit());
  vi.mocked(api.lookupScripts).mockResolvedValue({});
  vi.mocked(api.getTransactionActivity).mockResolvedValue(null);
  vi.mocked(api.getTransactionGraph).mockResolvedValue({ nodes: [], links: [] });
  vi.mocked(api.getTransactionCellDeps).mockResolvedValue([]);
  vi.mocked(api.getTransactionLifecycle).mockResolvedValue({
    proposedIn: null,
    commitmentWindow: null,
    commitmentDistance: null,
  } as unknown as Awaited<ReturnType<typeof api.getTransactionLifecycle>>);
});

/** The consumed and the created sheets, once the cells are drawn. */
async function sheets() {
  await screen.findByTestId('tx-io-input-0');
  const sheet = (side: 'consumed' | 'created') => {
    const el = document.querySelector<HTMLElement>(`[data-sheet="${side}"]`)!;
    return {
      el,
      tiles: Array.from(el.querySelectorAll<HTMLElement>('[data-cell-id]')).map(
        (t) => t.dataset.testid
      ),
      fold: el.querySelector<HTMLElement>(`[data-fold="${side}"]`),
    };
  };
  return { consumed: sheet('consumed'), created: sheet('created') };
}

describe('the transaction page folds a long sheet (lane D3)', () => {
  it('draws six consumed cells and one row that says the rest share the sender’s lock', async () => {
    render(<TransactionDetailPage />);
    const { consumed, created } = await sheets();
    expect(consumed.tiles).toEqual(Array.from({ length: 6 }, (_, i) => `tx-io-input-${i}`));
    expect(consumed.fold!.querySelector('[data-fold-words]')).toHaveTextContent(
      new RegExp(`^152 more cells from ${SHORT_SENDER}$`)
    );
    expect(created.tiles).toEqual(['tx-io-output-0', 'tx-io-output-1']);
    expect(created.fold).toBeNull();
    // the counts the page states are the transaction's, not the drawn tiles'
    expect(screen.getByRole('heading', { name: /^cells 158 in → 2 out$/i })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: '158 cells consumed' })).toBeInTheDocument();
    expect(document.body).toHaveTextContent('158 cells became 2.');
  });

  it('opens a sheet by one URL parameter, pushed so that back folds it again', async () => {
    render(<TransactionDetailPage />);
    const { consumed } = await sheets();
    fireEvent.click(within(consumed.fold!).getByRole('button', { name: /^all 158\scells ↓$/ }));
    expect(mockPush).toHaveBeenCalledWith(`/tx/${LONG_TX_HASH}?cells=consumed`, {
      scroll: false,
    });
    expect(mockReplace).not.toHaveBeenCalled();
  });

  it('draws every cell of a sheet the URL opens, and folds it by the same parameter', async () => {
    mockSearchParams = new URLSearchParams('cells=consumed');
    render(<TransactionDetailPage />);
    const { consumed } = await sheets();
    expect(consumed.tiles).toHaveLength(LONG_TX_INPUTS);
    fireEvent.click(within(consumed.fold!).getByRole('button', { name: 'fewer ↑' }));
    expect(mockPush).toHaveBeenCalledWith(`/tx/${LONG_TX_HASH}`, { scroll: false });
  });

  it('keeps the other parameters and the anchor when it opens or folds a sheet', async () => {
    mockSearchParams = new URLSearchParams('witness=0&cells=created');
    mockHash = `#${LONG_TX_PREV}-57`;
    render(<TransactionDetailPage />);
    const { consumed } = await sheets();
    fireEvent.click(within(consumed.fold!).getByRole('button', { name: /^all 158\scells ↓$/ }));
    expect(mockPush).toHaveBeenCalledWith(
      `/tx/${LONG_TX_HASH}?witness=0&cells=all#${LONG_TX_PREV}-57`,
      { scroll: false }
    );
  });

  it('never folds the cell the URL points at by its out point', async () => {
    mockHash = `#${LONG_TX_PREV}-57`;
    render(<TransactionDetailPage />);
    const { consumed } = await sheets();
    expect(consumed.tiles).toEqual([
      ...Array.from({ length: 6 }, (_, i) => `tx-io-input-${i}`),
      'tx-io-input-57',
    ]);
    expect(consumed.fold!.querySelector('[data-fold-words]')).toHaveTextContent(
      new RegExp(`^151 more cells from ${SHORT_SENDER}$`)
    );
  });

  it('refuses a cells parameter it does not know, naming the transaction', async () => {
    const quiet = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      mockSearchParams = new URLSearchParams('cells=everything');
      render(
        <ErrorBoundary fallbackRender={(error) => <p>crashed: {error.message}</p>}>
          <TransactionDetailPage />
        </ErrorBoundary>
      );
      expect(
        await screen.findByText(
          `crashed: Transaction ${LONG_TX_HASH}: cells=everything is none of consumed, created, all`
        )
      ).toBeInTheDocument();
    } finally {
      quiet.mockRestore();
    }
  });
});

// The developer tier's witnesses (lane D3; the board: "#1 to #157 are empty"): the long
// transaction listed 158 witness rows, 157 of them "#n · empty 0 B", about 6,300 px. A run of
// empty witnesses that no group reads folds to one line that says which they are, by counting; a
// witness with bytes is written whole, and the witness the URL selects is never in a run.
describe('the witnesses fold their runs of empty ones (lane D3)', () => {
  const rows = () =>
    Array.from(screen.getByTestId('tx-witness-tab').querySelectorAll<HTMLElement>('[data-testid]'))
      .map((el) => el.dataset.testid!)
      .filter((id) => /^tx-witness-(item|run)-/.test(id));

  it('writes the witness with bytes whole and the 157 empty ones as one line that opens', async () => {
    render(<TransactionDetailPage />);
    await screen.findByTestId('tx-witness-item-0');
    expect(rows()).toEqual(['tx-witness-item-0', 'tx-witness-run-1']);
    const run = screen.getByTestId('tx-witness-run-1');
    expect(run.querySelector('[data-run-words]')).toHaveTextContent(
      /^#1 to #157 are empty: 157\switnesses of 0\sB$/
    );
    fireEvent.click(within(run).getByRole('button', { name: /^all 158\switnesses ↓$/ }));
    expect(rows()).toEqual(
      Array.from({ length: LONG_TX_INPUTS }, (_, i) => `tx-witness-item-${i}`)
    );
    fireEvent.click(
      within(screen.getByTestId('tx-witness-tab')).getByRole('button', { name: 'fewer ↑' })
    );
    expect(rows()).toEqual(['tx-witness-item-0', 'tx-witness-run-1']);
  });

  it('never folds the witness the URL selects; a lone empty witness keeps its row', async () => {
    mockSearchParams = new URLSearchParams('witness=2');
    render(<TransactionDetailPage />);
    await screen.findByTestId('tx-witness-item-0');
    expect(rows()).toEqual([
      'tx-witness-item-0',
      'tx-witness-item-1',
      'tx-witness-item-2',
      'tx-witness-run-3',
    ]);
    expect(
      screen.getByTestId('tx-witness-run-3').querySelector('[data-run-words]')
    ).toHaveTextContent(/^#3 to #157 are empty: 155\switnesses of 0\sB$/);
    expect(screen.getByTestId('tx-witness-item-2')).toHaveAttribute('aria-pressed', 'true');
  });
});
