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

// Regression (2026-11-02, /script/0x9bd7… → Default Lock): an immutable family with one Type ID
// lineage held by an unspendable lock says "No one" in words, as before the timeline; it never
// draws a timeline nor fails on the holder's empty lock args.
describe('Script page · who can change immutable code (table-rows L1-3)', () => {
  beforeEach(() => {
    stubLayoutWidths({ chart: 900, list: 1248 });
    const [first] = fixture.family.history.filter((m) => m.event === 'deployed');
    const unspendable = {
      ...first,
      custody: {
        ...first.custody,
        class: 'unspendable',
        lockCodeHash: `0x${'00'.repeat(32)}`,
        lockHashType: 'data',
        lockArgs: '0x',
        threshold: null,
        keyCount: null,
        requireFirstN: null,
        keyHashes: [],
        verifiedByTx: null,
        witnessState: 'not_applicable',
      },
    };
    vi.mocked(api.getScriptFamilyDetail).mockResolvedValue({
      ...fixture.family,
      codeClasses: [
        { class: 'unspendable', lockCellsCount: 1, lockOwnedCapacitySum: '1', referenceCount: 1 },
      ],
      history: [unspendable],
    } as never);
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

  // Lane F follow-up: the hero line names immutable code as its accent tag, the hint in its title.
  it('tags the hero line "immutable" as the accent, why in its title', async () => {
    await renderJoyid();
    const hero = screen.getByRole('region', { name: 'JoyID' });
    const tag = within(hero).getByText('immutable');
    expect(tag).toHaveAttribute('data-row-tag', 'accent');
    expect(tag).toHaveClass('text-jade');
    expect(tag).toHaveAttribute('title', 'immutable · code cell unspendable');
    expect(tag.parentElement).toHaveTextContent(/^lock script · immutable$/);
    expect(hero.querySelectorAll('h1.text-text-bright')).toHaveLength(1);
  });

  it('says no one can replace it, without a holder timeline', async () => {
    await renderJoyid();
    const section = screen.getByRole('region', { name: 'Who can change its code' });
    expect(within(section).queryByTestId('holder-timeline')).toBeNull();
    expect(section.querySelector('[data-role="caption"]')).toHaveTextContent(
      /^No one\. JoyID cells name their code by its data hash, or through a code cell that can never be spent/
    );
  });
});
