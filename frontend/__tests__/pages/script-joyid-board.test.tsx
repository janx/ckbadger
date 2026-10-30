import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { screen, waitFor, within } from '@testing-library/react';
import { render } from '../utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import fixture from '@/__tests__/fixtures/scripts/joyid-family-detail.json';
import ScriptDetailPage from '@/app/scripts/[name]/client-page';
import { api, type Cell } from '@/lib/api';
import { DATA_COLOR, HELD_CAPACITY_COLOR } from '@/lib/design-tokens';

vi.mock('@/lib/api', () => ({
  api: {
    getScriptFamilyDetail: vi.fn(),
    getScriptUsage: vi.fn(),
    getScriptCapacityChart: vi.fn(),
    getScriptCapacityChartByCodeHash: vi.fn(),
    getCell: vi.fn(),
    getCellsByScriptRef: vi.fn(),
    getCodeCells: vi.fn(),
    lookupScripts: vi.fn(),
  },
  isNotFoundApiError: () => false,
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div /> }));
vi.mock('@/src/navigation', () => ({
  useParams: () => ({ name: 'JoyID' }),
  usePathname: () => '/scripts/JoyID',
  useSearchParams: () => new URLSearchParams(),
}));

// JoyID as the chain has it (ScriptJoyid board, V80): five binaries under one Type ID, held by
// three multisigs; the API shapes (versions, history markers, references) are /scripts/JoyID's.
const codeCell = (txHash: string, outputIndex: number): Cell => {
  const v = fixture.versions.find((x) => x.tx === txHash && x.idx === outputIndex);
  if (!v) throw new Error(`unexpected code cell ${txHash}:${outputIndex}`);
  return {
    txHash,
    outputIndex,
    capacity: String(v.cap),
    commonKnowledgeSize: String(v.cap),
    lockScriptHash: '0x01',
    dataSize: v.size,
    createdAtBlock: v.block,
    status: v.v === 5 ? 'live' : 'dead',
  };
};

async function renderJoyid() {
  render(<ScriptDetailPage name="JoyID" />);
  await screen.findByRole('heading', { name: 'JoyID', level: 1 });
  await screen.findByRole('table', { name: 'Its cells' });
}

describe('JoyID script page against the ScriptJoyid board', () => {
  beforeEach(() => {
    stubChartWidth(900);
    vi.mocked(api.getScriptFamilyDetail).mockResolvedValue(fixture.family as never);
    vi.mocked(api.getScriptUsage).mockResolvedValue(fixture.usage);
    vi.mocked(api.getScriptCapacityChart).mockResolvedValue(fixture.chart);
    vi.mocked(api.getCell).mockImplementation(async (tx, index) => codeCell(tx, index));
    vi.mocked(api.getCellsByScriptRef).mockResolvedValue(fixture.cells as never);
    vi.mocked(api.lookupScripts).mockResolvedValue({});
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('opens with breadcrumb, role, a summary sentence and a facts rail (SC-S1)', async () => {
    await renderJoyid();
    expect(screen.getByRole('navigation', { name: 'Breadcrumb' })).toHaveTextContent(
      'Scripts / Locks'
    );
    expect(screen.getByText('lock script · upgradable')).toBeInTheDocument();
    const summary = screen.getByTestId('script-summary').textContent!;
    expect(summary).toContain(
      'JoyID decides who can spend 161,049 live cells holding 456,002,179.57'
    );
    expect(summary).toContain('2,707,778 cells have used it since 2023-10-07');
    expect(summary).toContain('it has been replaced 4 times, last on 2025-11-21');
    expect(summary).toContain('now sits under a multisig that has never signed');
    const rail = screen.getByRole('complementary', { name: 'Script facts' });
    expect(rail).toHaveTextContent('of 2,707,778 that ever used it');
    expect(rail).toHaveTextContent('Upgradable · 5 versions');
    expect(rail).toHaveTextContent('last replaced 2025-11-21');
    expect(rail).toHaveTextContent('multisig 0x6598…d696');
    expect(rail).toHaveTextContent('has never signed');
    expect(rail).toHaveTextContent('In use since2023-10-07');
  });

  it('draws History with occupied in jade, a separate occupied panel and the code track (SC-S2, SC-S3)', async () => {
    const { container } = { container: document.body };
    await renderJoyid();
    const fills = [...container.querySelectorAll('svg path[fill]')].map((p) =>
      p.getAttribute('fill')
    );
    expect(fills).toContain(DATA_COLOR);
    expect(fills).toContain(HELD_CAPACITY_COLOR);
    expect(fills).not.toContain('#58a5b5');
    expect(
      screen.getByText("Occupied by those cells' own bytes", { selector: 'p' })
    ).toBeInTheDocument();
    const reading = screen.getByTestId('script-history-reading');
    expect(reading).toHaveTextContent('2026-10-29');
    expect(reading).toHaveTextContent('CKB held 456,002,179.57');
    expect(reading).toHaveTextContent('code v5 held by multisig 0x6598…d696');
    const track = screen.getByLabelText('Code track');
    expect(within(track).getByText('v1 · v2')).toBeInTheDocument();
    expect(within(track).getByText('v3 · v4')).toBeInTheDocument();
    expect(within(track).getByText('v5')).toBeInTheDocument();
    expect(track).toHaveTextContent('multisig 0xd1db…b95c · 3 of 5 signatures');
    expect(track).toHaveTextContent('multisig 0x6598…d696 · signers not on chain yet');
    expect(document.body.textContent).not.toMatch(/common knowledge/i);
  });

  it('shows who can change the code as three multisigs, key by key (SC-S4)', async () => {
    await renderJoyid();
    const holders = screen.getByRole('list', { name: 'Code cell holders' });
    // one column at 390 px, one per holder from md up (no horizontal overflow on phones)
    expect(holders).toHaveClass('grid-cols-1');
    expect(holders.style.getPropertyValue('--holders')).toBe('3');
    const columns = within(holders)
      .getAllByRole('listitem')
      .filter((li) => li.dataset.holder);
    expect(columns).toHaveLength(3);
    expect(columns[0]).toHaveTextContent('2023-10-07 → 2025-08-28');
    expect(columns[0]).toHaveTextContent('3 of 5 signatures');
    expect(columns[0]).toHaveTextContent('signed v2 and v3');
    expect(columns[1]).toHaveTextContent('signed v4 and v5');
    expect(columns[2]).toHaveTextContent('has never signed');
    expect(columns[2]).toHaveTextContent(
      'Its keys and threshold appear on chain the first time it signs.'
    );
    expect(columns[0].querySelectorAll('[data-kept="true"]')).toHaveLength(2);
    expect(columns[1].querySelectorAll('[data-kept="true"]')).toHaveLength(2);
    expect(
      screen.getByText(/kept across the handover on 2025-08-28: 2 of 5 keys/)
    ).toBeInTheDocument();
    expect(screen.getByText(/Three multisigs have held it\./)).toBeInTheDocument();
  });

  it('merges the versions into one full-width table (SC-S5)', async () => {
    await renderJoyid();
    expect(screen.queryByRole('table', { name: 'Code cell deployments' })).toBeNull();
    const table = screen.getByRole('table', { name: 'Code versions' });
    const rows = within(table).getAllByRole('row').slice(1);
    expect(rows.map((row) => row.textContent!.match(/^v\d/)![0])).toEqual([
      'v1',
      'v2',
      'v3',
      'v4',
      'v5',
    ]);
    await waitFor(() => expect(rows[0]).toHaveTextContent('96,760 B'));
    expect(rows[0]).toHaveTextContent('2023-10-07 15:39');
    expect(rows[0]).toHaveTextContent('#11,132,140');
    expect(rows[0]).toHaveTextContent('consumed by v2');
    expect(rows[0]).toHaveTextContent('first deployed');
    expect(rows[0]).toHaveTextContent('into multisig 0xd1db…b95c');
    expect(rows[2]).toHaveTextContent('hands it to multisig 0x07a2…68ca');
    expect(rows[4]).toHaveTextContent('current');
    expect(rows[4]).toHaveTextContent('100,438.00000000');
    expect(rows[4]).toHaveTextContent('live');
    expect(screen.getByText(/one type id, 0xd00c84f0…318323, 5 binaries/)).toBeInTheDocument();
  });

  it('says how to use it in a transaction (SC-S6)', async () => {
    await renderJoyid();
    const section = screen
      .getByRole('heading', { name: 'Use it in a transaction' })
      .closest('section')!;
    expect(section).toHaveTextContent(
      '0xd00c84f0ec8fd441c38bc3f87a371f547190f2fcff88e642bc5bf54b9e318323'
    );
    expect(section).toHaveTextContent('type · follows every upgrade');
    expect(section).toHaveTextContent(
      '0x673e1843035ce9c249efc36b62865e7a7c9f6e2ca6fc9c66fd108307e81355f9:0'
    );
    expect(section).toHaveTextContent('Pin this exact binary instead');
    expect(section).toHaveTextContent(
      '0x4af7d832e7c328aa4ad6f2deb669743446aea7b9a2f0c25646897501ac4f1ebe'
    );
  });

  it('lists all its cells with type names, free in the anatomy and occupied bytes (SC-S7, SC-S10)', async () => {
    await renderJoyid();
    expect(screen.getByTestId('script-cells-summary')).toHaveTextContent(
      '161,049 live cells use JoyID as their lock'
    );
    const table = screen.getByRole('table', { name: 'Its cells' });
    const rows = within(table).getAllByRole('row').slice(1);
    expect(rows[0]).toHaveTextContent('0x44e5e9…517f:1');
    expect(rows[0]).toHaveTextContent('63 B');
    expect(rows[0]).toHaveTextContent('—');
    expect(rows[1]).toHaveTextContent('COTA');
    expect(rows[1]).not.toHaveTextContent('0x1122a4fb');
    expect(rows[0].querySelector('[data-part="free"]')).not.toBeNull();
    expect(rows[0].innerHTML).not.toMatch(/#f2c55c|text-gold|text-dao/);
    expect(rows[0].textContent).not.toContain(
      'ckb1qrgqep8saj8agswr30pls73hra28ry8jlnlc3ejzh3dl2ju7xxpjxqgqq03dwzcucjtl5qcg72kxx9euah4avhq3dv77xsf8'
    );
    // Every registered reference is listed, not only a selected version's.
    expect(api.getCellsByScriptRef).toHaveBeenCalledWith(
      expect.objectContaining({
        codeHash: '0xd00c84f0ec8fd441c38bc3f87a371f547190f2fcff88e642bc5bf54b9e318323',
        hashType: 'type',
        scriptKind: 'lock',
      })
    );
  });

  it('keeps registered references in the developer tier (SC-S9)', async () => {
    await renderJoyid();
    const tier1 = document.querySelector('[data-tier="1"]') as HTMLElement;
    const tier2 = document.querySelector('[data-tier="2"]') as HTMLElement;
    expect(within(tier1).queryByRole('heading', { name: 'Registered references' })).toBeNull();
    expect(
      within(tier2).getByRole('heading', { name: 'Registered references' })
    ).toBeInTheDocument();
  });
});
