import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor, within, fireEvent } from '@testing-library/react';
import { render } from '../utils/test-utils';
import ScriptsPage from '@/app/scripts/page';
import { api, type KnownScript } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: { getScripts: vi.fn() },
  isWarmupPendingError: () => false,
  isNetworkInitializingError: vi.fn(() => false),
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div /> }));

// Rows shaped like /scripts family list items after scriptFamilyListItemToKnownScript.
function family(overrides: Partial<KnownScript>): KnownScript {
  return {
    codeHash: 'family:x',
    name: 'X',
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
    ownedCapacitySum: '100000000',
    ownedKnowledgeSum: '50000000',
    liveCellsCount: 1,
    cellsCount: 2,
    versionsCount: 1,
    codeClasses: [],
    firstDeployedAt: null,
    ...overrides,
  };
}

const joyid = family({
  codeHash: 'family:joyid',
  name: 'JoyID',
  description: 'An universal Account Protocol for Web3 Mass-adoption',
  ownedCapacitySum: '45600217957000000',
  ownedKnowledgeSum: '1909628300000000',
  liveCellsCount: 161049,
  cellsCount: 2707778,
  versionsCount: 5,
  firstDeployedAt: {
    blockNumber: 11132140,
    timestampMs: 1696664371829,
    versionHash: '0x0b6a8ef9dee12629aaa738266a56e6e0ec3330782af9ac29f50e04b0daa1845c',
  },
  codeClasses: [
    {
      class: 'multisig',
      lockCellsCount: 161049,
      lockOwnedCapacitySum: '45600217957000000',
      referenceCount: 1,
    },
  ],
});
const defaultLock = family({
  codeHash: 'family:default-lock',
  name: 'Default Lock',
  ownedCapacitySum: '4100000000000000000',
  ownedKnowledgeSum: '50000000000000000',
  liveCellsCount: 1717237,
  cellsCount: 78239443,
  versionsCount: 2,
  firstDeployedAt: {
    blockNumber: 0,
    timestampMs: 1573852190812,
    versionHash: '0x709f3fda12f561cfacf92273c57a98fede188a3f1a59b1f888d113f9cce08649',
  },
  codeClasses: [
    { class: 'fixed', lockCellsCount: 1, lockOwnedCapacitySum: '1', referenceCount: 1 },
    { class: 'unspendable', lockCellsCount: 2, lockOwnedCapacitySum: '2', referenceCount: 1 },
  ],
});
const godwoken = family({
  codeHash: 'family:godwoken',
  name: 'Godwoken Custodian Lock',
  deprecated: true,
  ownedCapacitySum: '10000000000000',
  ownedKnowledgeSum: '10000000',
  codeClasses: [],
});

const response = {
  data: [joyid, defaultLock, godwoken],
  total: 3,
  limit: 50,
  hasMore: false,
  nextCursor: null,
  roleCounts: { all: 3, locks: 3, types: 0 },
};

describe('Scripts list against the Scripts board', () => {
  beforeEach(() => {
    vi.mocked(api.getScripts).mockResolvedValue(response);
  });

  it('opens with the board lead sentence and a name search (SC-L5)', async () => {
    render(<ScriptsPage />);
    expect(
      screen.getByText(
        'Every cell runs one lock script, which decides who can spend it, and may run one type script, which decides what the cell is and how it may change.'
      )
    ).toBeInTheDocument();
    expect(screen.getByPlaceholderText('Search by name or code hash')).toBeInTheDocument();
  });

  it('writes the Code column as Immutable or Upgradable · N versions · by whom (SC-L2, SC-L4)', async () => {
    render(<ScriptsPage />);
    const table = await screen.findByRole('table', { name: 'Scripts' });
    const row = (name: string) =>
      within(table).getByRole('link', { name }).closest('tr') as HTMLElement;
    const joyCode = within(row('JoyID')).getByTestId('script-code-class');
    expect(joyCode.textContent).toContain('Upgradable · 5 versions');
    expect(joyCode.textContent).toContain('by a multisig');
    expect(within(row('Default Lock')).getByTestId('script-code-class').textContent).toContain(
      'Immutable'
    );
    expect(table.textContent).not.toMatch(/Fixed bytecode|code classes/);
    // No class evidence from the API stays visible as such, never invented.
    expect(
      within(row('Godwoken Custodian Lock')).getByTestId('script-code-class').textContent
    ).toBe('Code evidence unavailable');
  });

  it('shows "of N ever" and exact CKB, and a rounded occupied share (SC-L6, SC-L7)', async () => {
    render(<ScriptsPage />);
    const table = await screen.findByRole('table', { name: 'Scripts' });
    const joyRow = within(table).getByRole('link', { name: 'JoyID' }).closest('tr') as HTMLElement;
    expect(joyRow.textContent).toContain('161,049');
    expect(joyRow.textContent).toContain('of 2,707,778 ever');
    expect(joyRow.textContent).toContain('456,002,179.57');
    expect(joyRow.textContent).not.toContain('456.00M');
    expect(joyRow.textContent).toContain('4.19%');
    const godRow = within(table)
      .getByRole('link', { name: 'Godwoken Custodian Lock' })
      .closest('tr') as HTMLElement;
    expect(godRow.textContent).toContain('0.0001%');
    expect(godRow.textContent).not.toContain('0.00%');
  });

  it('marks deprecated in grey, not red (SC-L8)', async () => {
    render(<ScriptsPage />);
    const table = await screen.findByRole('table', { name: 'Scripts' });
    const mark = within(table).getByText('deprecated');
    expect(mark.className).not.toMatch(/negative|rouge/);
    expect(mark.className).toContain('text-text-dim');
  });

  it('uses underline role tabs and one "sorted by" control in board words (SC-L8)', async () => {
    render(<ScriptsPage />);
    const tabs = await screen.findByRole('group', { name: 'Role' });
    const all = within(tabs).getByRole('button', { name: /All/ });
    expect(all).toHaveAttribute('aria-pressed', 'true');
    expect(all.className).toContain('border-b-2');
    expect(screen.queryByText('Used (CKB)')).toBeNull();
    expect(screen.queryByText('Capacity (CKB)')).toBeNull();
    const sort = screen.getByRole('combobox', { name: 'Sort scripts' });
    expect(sort).toHaveValue('capacity');
    expect(within(sort).getByRole('option', { name: 'CKB in their cells' })).toBeInTheDocument();
    fireEvent.change(sort, { target: { value: 'liveCells' } });
    await waitFor(() =>
      expect(api.getScripts).toHaveBeenLastCalledWith(
        expect.objectContaining({ sortKey: 'liveCells', sortDirection: 'desc' })
      )
    );
  });

  it('never says "Common Knowledge" in the narrow row (SC-L8)', async () => {
    render(<ScriptsPage />);
    await screen.findByRole('table', { name: 'Scripts' });
    expect(document.body.textContent).not.toMatch(/Common Knowledge/i);
  });
});

describe('Scripts list · visual pass (R2b-b)', () => {
  beforeEach(() => {
    vi.mocked(api.getScripts).mockResolvedValue(response);
  });

  it('dates the code: "since" for upgradable code, "genesis" for genesis code (SC-L4)', async () => {
    render(<ScriptsPage />);
    const table = await screen.findByRole('table', { name: 'Scripts' });
    const code = (name: string) =>
      within(within(table).getByRole('link', { name }).closest('tr') as HTMLElement).getByTestId(
        'script-code-class'
      );
    expect(code('JoyID')).toHaveTextContent('by a multisig · since 2023-10-07');
    expect(code('Default Lock')).toHaveTextContent('genesis · code cell unspendable');
    expect(code('Godwoken Custodian Lock')).toHaveTextContent('Code evidence unavailable');
  });

  it('searches by code hash through the API (SC-L5)', async () => {
    render(<ScriptsPage />);
    fireEvent.change(screen.getByRole('textbox', { name: 'Search scripts' }), {
      target: { value: ' 0xd00c84f0 ' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    await waitFor(() =>
      expect(api.getScripts).toHaveBeenLastCalledWith(
        expect.objectContaining({ search: '0xd00c84f0' })
      )
    );
  });

  it('has no second "Scripts" heading over the table (board: one sort line)', async () => {
    render(<ScriptsPage />);
    await screen.findByRole('table', { name: 'Scripts' });
    expect(screen.queryByRole('heading', { name: 'Scripts', level: 2 })).toBeNull();
    expect(screen.getByText('3 scripts')).toBeInTheDocument();
  });
});
