import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { screen, waitFor, within } from '@testing-library/react';
import { render } from '../utils/test-utils';
import { stubLayoutWidths } from '@/__tests__/utils/phone-row';
import fixture from '@/__tests__/fixtures/scripts/joyid-family-detail.json';
import { byScript, rowDetail } from '@/__tests__/utils/script-newest-cells';
import ScriptDetailPage from '@/app/scripts/[name]/client-page';
import { api, type Cell } from '@/lib/api';
import { expectPrompt, promptNote } from '@/__tests__/utils/prompt-heading';

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

// Table-rows plan L1-4 (board ScriptDetail, T1/T8): "Code versions" is one 36 px line per version,
// newest first — Version · Deployed · Block · Code cell · Held by · Status (and the live cells that
// use it); the exact time, the binary, the data hash, the code cell's CKB and who signed it are the
// row's title.
async function versions() {
  render(<ScriptDetailPage name="JoyID" />);
  const table = await screen.findByRole('table', { name: 'Code versions' });
  const rows = within(table).getAllByRole('row').slice(1);
  return { table, rows };
}
const cells = (row: HTMLElement) => [...row.querySelectorAll('td')] as HTMLElement[];

describe('Code versions rows (table-rows L1-4)', () => {
  beforeEach(() => {
    stubLayoutWidths({ chart: 900, list: 1248 });
    vi.mocked(api.getScriptFamilyDetail).mockResolvedValue(fixture.family as never);
    vi.mocked(api.getScriptUsage).mockResolvedValue(fixture.usage);
    vi.mocked(api.getScriptCapacityChart).mockResolvedValue(fixture.chart);
    // Lane S: the newest cells read the same fixture rows newest first, and each row's detail.
    vi.mocked(api.getCell).mockImplementation(
      async (tx, index) => rowDetail(fixture.cells.data as never, tx, index) ?? codeCell(tx, index)
    );
    vi.mocked(api.getCellsByScriptRef).mockImplementation(byScript(() => fixture.cells as never));
    vi.mocked(api.lookupScripts).mockResolvedValue({});
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('lists the versions newest first, one 36 px line each, under "newest first"', async () => {
    const { table, rows } = await versions();
    expect(
      within(table)
        .getAllByRole('columnheader')
        .map((h) => h.textContent)
    ).toEqual(['Version', 'Deployed', 'Block', 'Code cell', 'Held by', 'Status', 'Live cells']);
    expect(rows.map((row) => cells(row)[0].textContent)).toEqual(['v5', 'v4', 'v3', 'v2', 'v1']);
    // T5: column heads at 11 px.
    for (const h of within(table).getAllByRole('columnheader'))
      expect(h.firstElementChild).toHaveClass('text-meta');
    for (const td of cells(rows[0])) {
      expect(td).toHaveClass('py-[9px]', 'align-middle');
      expect(td.querySelector('.block, br')).toBeNull();
    }
    // Phase 5 (A18): the versions command runs on their count; "newest first" is its note.
    const heading = screen.getByRole('heading', { name: 'Versions 5', level: 2 });
    expectPrompt(heading, 'Versions', '5');
    expect(promptNote(heading)).toHaveTextContent(/^newest first$/);
  });

  it('writes v5 as the board: bright version linking its code cell, date, block, outpoint, holder, live · current', async () => {
    const { rows } = await versions();
    const [version, deployed, block, cell, held, status, live] = cells(rows[0]);
    const link = within(version).getByRole('link', { name: 'v5' });
    expect(link).toHaveClass('text-body', 'text-text-bright', 'leading-[18px]');
    expect(link).toHaveAttribute(
      'href',
      '/mainnet/cell/0x673e1843035ce9c249efc36b62865e7a7c9f6e2ca6fc9c66fd108307e81355f9-0'
    );
    expect(deployed).toHaveTextContent(/^2025-11-21$/);
    expect(block).toHaveTextContent(/^#17,894,900$/);
    expect(within(block).getByRole('link')).toHaveClass('text-text-dim');
    expect(cell).toHaveTextContent(/^0x673e18…55f9:0$/);
    expect(cell.firstElementChild).toHaveAttribute(
      'title',
      '0x673e1843035ce9c249efc36b62865e7a7c9f6e2ca6fc9c66fd108307e81355f9:0'
    );
    // The out point opens the code cell too, in dim ink (T5: the version stays the bright thing).
    const outpoint = within(cell).getByRole('link', { name: '0x673e18…55f9:0' });
    expect(outpoint).toHaveAttribute(
      'href',
      '/mainnet/cell/0x673e1843035ce9c249efc36b62865e7a7c9f6e2ca6fc9c66fd108307e81355f9-0'
    );
    expect(outpoint).toHaveClass('text-text-dim');
    expect(held).toHaveTextContent(/^multisig 0x6598…d696$/);
    // T5: one bright thing a row — the version; the rest in body ink or dim.
    expect(rows[0]).toHaveClass('text-text');
    expect(rows[0].querySelectorAll('.text-text-bright')).toHaveLength(1);
    const tag = status.querySelector<HTMLElement>('[data-row-tag]')!;
    expect(tag.dataset.rowTag).toBe('accent');
    expect(tag).toHaveTextContent('live · current');
    await waitFor(() => expect(live).toHaveTextContent('161,049'));
  });

  it('marks a replaced version "replaced at #N" in dim ink, held by the holder of its code cell', async () => {
    const { rows } = await versions();
    const v4 = cells(rows[1]);
    expect(v4[4]).toHaveTextContent(/^multisig 0x07a2…68ca$/);
    expect(v4[5]).toHaveTextContent(/^replaced at #17,894,900$/);
    expect(v4[5].firstElementChild).toHaveClass('text-text-dim');
    // Lane L: the block that replaced it is a link.
    expect(within(v4[5]).getByRole('link', { name: '#17,894,900' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/17894900'
    );
    expect(v4[5].querySelector('[data-row-tag]')).toBeNull();
    expect(cells(rows[4])[4]).toHaveTextContent(/^multisig 0xd1db…b95c$/);
    expect(cells(rows[4])[5]).toHaveTextContent(/^replaced at #11,133,523$/);
  });

  it('keeps every field that left the row in its title (nothing deleted)', async () => {
    const { rows } = await versions();
    const v1 = rows[4];
    await waitFor(() => expect(v1.getAttribute('title')).toContain('96,760 B binary'));
    const title = v1.getAttribute('title')!;
    for (const part of [
      'v1',
      'deployed 2023-10-07 15:39 (UTC+8) at #11,132,140',
      'code cell 0xbac0f6f0a177a03974870df47ab0c3e761c5dd19e9ca165358d0b72bd7f433f5:0',
      '96,886.00000000 CKB',
      `data hash ${fixture.family.versions.find((v) => v.deployments[0].createdAtBlock === 11132140)!.versionHash}`,
      'first deployed into multisig 0xd1db…b95c',
      'held by multisig 0xd1db…b95c',
      'consumed by v2',
    ])
      expect(title).toContain(part);
    expect(rows[2].getAttribute('title')).toContain(
      'signed by multisig 0xd1db…b95c, hands it to 0x07a2…68ca'
    );
    expect(rows[0].getAttribute('title')).toContain('live · current');
  });

  it('keeps each version within 90 visible characters (T6)', async () => {
    const { rows } = await versions();
    await waitFor(() => expect(rows[0]).toHaveTextContent('161,049'));
    for (const row of rows)
      expect(row.textContent!.replace(/\s+/g, ' ').trim().length).toBeLessThanOrEqual(90);
  });
});
