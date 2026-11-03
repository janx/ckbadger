import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { screen, waitFor, within } from '@testing-library/react';
import { render } from '../utils/test-utils';
import { stubLayoutWidths } from '@/__tests__/utils/phone-row';
import fixture from '@/__tests__/fixtures/scripts/joyid-family-detail.json';
import { byScript, rowDetail } from '@/__tests__/utils/script-newest-cells';
import ScriptDetailPage from '@/app/scripts/[name]/client-page';
import { api, type Cell } from '@/lib/api';

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

async function renderJoyid() {
  render(<ScriptDetailPage name="JoyID" />);
  await screen.findByRole('heading', { name: 'JoyID', level: 1 });
  await screen.findByRole('table', { name: 'Its cells' });
}
const tier2 = () => document.querySelector('[data-tier="2"]') as HTMLElement;

// Table-rows plan L1-5 (board ScriptDetail): "Its cells" keeps its one-line rows with the key cell
// at 13 px bright (T5); tier 2 is trimmed to the board's two blocks — "How to read this page" as
// one paragraph and "The script" as one definition list per reference. Every sentence and fact the
// four former sections held is in one of the two.
describe('Script page · Its cells and tier 2 (table-rows L1-5)', () => {
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

  it('leads each of its cells with the cell at 13 px bright, the rest of the row in body ink (T5)', async () => {
    await renderJoyid();
    const table = screen.getByRole('table', { name: 'Its cells' });
    const [row] = within(table).getAllByRole('row').slice(1);
    const key = within(row).getByRole('link', { name: '0x44e5e9…517f:1' });
    expect(key).toHaveClass('text-body', 'text-text-bright', 'leading-[18px]');
    expect(row).toHaveClass('text-text');
    expect(row.querySelectorAll('.text-text-bright')).toHaveLength(1);
    // T5: column heads at 11 px.
    for (const h of within(table).getAllByRole('columnheader'))
      expect(h.firstElementChild).toHaveClass('text-meta');
  });

  it('keeps tier 2 to the two board blocks: one paragraph and the script facts', async () => {
    await renderJoyid();
    const headings = within(tier2())
      .getAllByRole('heading', { level: 2 })
      .map((h) => h.textContent);
    expect(headings).toEqual(['How to read this page', 'The script']);
    const read = within(tier2())
      .getByRole('heading', { name: 'How to read this page' })
      .closest('section')!;
    expect(read.querySelectorAll('p')).toHaveLength(1);
    const paragraph = read.querySelector('p')!;
    // Every sentence the former sections wrote, in one paragraph.
    expect(within(paragraph).getByTestId('script-summary')).toHaveTextContent(
      'JoyID decides who can spend 161,049 live cells'
    );
    expect(within(paragraph).getByTestId('script-history-peak')).toHaveTextContent(
      /CKB held peaked at 456,002,179\.570* CKB on 2026-10-29/
    );
    expect(paragraph).toHaveTextContent('A cell must hold enough CKB to pay for its own bytes.');
    expect(paragraph).toHaveTextContent(
      "JoyID cells point at their code through one code cell's type id, not at a fixed binary."
    );
    expect(paragraph).toHaveTextContent(
      'Code versions: one type id, 0xd00c84f0…318323, 5 binaries.'
    );
  });

  // Lane E (C6): one map of the script's shape and one row per reference, instead of one definition
  // list per reference; every fact of the former lists is in a row, its title, or the fold below.
  it('draws the script once and gives each reference one row, the full facts folded', async () => {
    await renderJoyid();
    const facts = within(tier2()).getByRole('heading', { name: 'The script' }).closest('section')!;
    expect(within(facts).getByTestId('script-cells-summary')).toHaveTextContent(
      '161,049 live cells use JoyID as their lock'
    );
    expect(facts).toHaveTextContent('Script · 33 bytes, then the args each cell sets');
    expect(
      [...within(facts).getByRole('group', { name: 'lock script byte map' }).children].map(
        (p) => (p as HTMLElement).dataset.segment
      )
    ).toEqual(['code_hash', 'hash_type']);
    const table = within(facts).getByRole('table', { name: 'Registered references' });
    const rows = within(table).getAllByRole('row').slice(1);
    expect(rows).toHaveLength(fixture.family.references.length);
    expect(rows[0]).toHaveTextContent('type');
    expect(rows[0]).toHaveTextContent('161,049');
    expect(rows[0]).toHaveAttribute(
      'title',
      expect.stringContaining('0xd00c84f0ec8fd441c38bc3f87a371f547190f2fcff88e642bc5bf54b9e318323')
    );
    expect(within(rows[0]).getByRole('link', { name: /0x673e18…55f9:0/ })).toHaveAttribute(
      'href',
      '/mainnet/cell/0x673e1843035ce9c249efc36b62865e7a7c9f6e2ca6fc9c66fd108307e81355f9-0'
    );
    await waitFor(() => expect(rows[0]).toHaveTextContent('A multisig can replace'));
    expect(facts).toHaveTextContent(
      'A type reference follows every upgrade; its data hash with hash type data1 or data2 pins this exact binary; an upgrade will not reach cells that pin it.'
    );
    // the former definition list, per reference, in one fold
    const full = within(facts).getByText('Each reference in full').closest('details')!;
    expect(full).not.toHaveAttribute('open');
    const list = full.querySelector('dl')!;
    expect(
      within(list)
        .getAllByRole('term')
        .map((t) => t.textContent)
    ).toEqual(['code hash', 'hash type', 'current code', 'pin instead', 'cell dep', 'held by']);
    const dd = (term: string) =>
      within(list)
        .getAllByRole('term')
        .find((t) => t.textContent === term)!.nextElementSibling as HTMLElement;
    expect(dd('code hash')).toHaveTextContent(
      /^0xd00c84f0ec8fd441c38bc3f87a371f547190f2fcff88e642bc5bf54b9e318323$/
    );
    expect(dd('hash type')).toHaveTextContent(
      /^type · follows every upgrade · 161,049 live cells$/
    );
    expect(dd('current code')).toHaveTextContent(
      /^0x4af7d832e7c328aa4ad6f2deb669743446aea7b9a2f0c25646897501ac4f1ebe$/
    );
    expect(dd('cell dep')).toHaveTextContent(
      '0x673e1843035ce9c249efc36b62865e7a7c9f6e2ca6fc9c66fd108307e81355f9:0 · dep type code · the live code cell of the current version'
    );
    expect(dd('held by')).toHaveTextContent('Lock args 0x65983f1d7520543a6f182a50354fb4ae2deed696');
  });
});
