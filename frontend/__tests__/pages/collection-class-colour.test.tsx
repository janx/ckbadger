import { render, screen, within } from '@/__tests__/utils/test-utils';
import IdentityCollectionPage from '@/app/identities/[collectionId]/client-page';
import ClusterPage from '@/app/clusters/[clusterId]/client-page';
import { api, type AssetCapacityHistory } from '@/lib/api';
import { mockItemReads } from '@/__tests__/utils/item-read-fixtures';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import {
  clusterDetailProducer,
  collectionInsightsProducer,
} from '@/__tests__/msw/fixtures/item-producer';
import { CLASS_COLORS } from '@/lib/design-tokens';

vi.mock('@/components/layout/header', () => ({ Header: () => null }));

// PB-12 (UI review 2026-11-14): a collection page draws its history's weekly bars and its holders'
// bars in its own class colour — identity rose on an identity collection, not the objects' lotus
// violet and the interactive azure; object violet on a cluster.

const day = (offset: number) => new Date(Date.UTC(2023, 9, 25 + offset)).toISOString().slice(0, 10);

/** 21 complete days, 2023-10-25 … 2023-11-14, and the two whole weeks inside them. */
function history() {
  const chart: AssetCapacityHistory = {
    source: 'indexedChainTimeUtc8',
    completeThrough: day(20),
    anchor: { blockNumber: 100, blockHash: '0x' + 'aa'.repeat(32), chainTime: '1700000000000' },
    title: 'Collection capacity',
    data: Array.from({ length: 21 }, (_, i) => ({
      date: day(i),
      values: { used: String(100000000 * (i + 1)), unused: String(200000000 * (i + 1)) },
    })),
    series: [
      { key: 'used', label: 'Used', color: '#f59e0b' },
      { key: 'unused', label: 'Unused', color: '#00c389' },
    ],
  };
  vi.spyOn(api, 'getIdentityCollectionCapacityChart').mockResolvedValue(chart);
  vi.spyOn(api, 'getSporeClusterCapacityChart').mockResolvedValue(chart);
  vi.spyOn(api, 'getCollectionInsights').mockResolvedValue({
    ...collectionInsightsProducer,
    completeThrough: day(20),
    weeklyActivity: [
      { weekStart: day(5), mintTransactions: 0, transferTransactions: 7, burnTransactions: 0 },
      { weekStart: day(12), mintTransactions: 0, transferTransactions: 3, burnTransactions: 0 },
    ],
  });
}

beforeEach(() => {
  mockItemReads();
  history();
  stubChartWidth(1248);
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

/** The fills of the weekly bars in the History region. */
async function weeklyBarFills() {
  const region = await screen.findByRole('region', { name: 'History' });
  const chart = await within(region).findByRole('img', { name: /per week/ });
  return new Set(
    Array.from(chart.querySelectorAll('rect[fill^="var(--c-"]')).map((r) => r.getAttribute('fill'))
  );
}

/** The fills of the holder bars in "Who holds it". */
async function holderBarFills() {
  const region = await screen.findByRole('region', { name: 'Who holds it' });
  const table = await within(region).findByRole('table', { name: 'Top 10 collection holders' });
  const segments = Array.from(table.querySelectorAll<HTMLElement>('[data-segment]'));
  expect(segments.length).toBeGreaterThan(0);
  return new Set(segments.map((s) => s.style.backgroundColor));
}

it('draws an identity collection in the identity rose', async () => {
  render(<IdentityCollectionPage collectionId="dotbit" />);
  expect(await weeklyBarFills()).toEqual(new Set([CLASS_COLORS.identities]));
  expect(await holderBarFills()).toEqual(new Set([CLASS_COLORS.identities]));
});

it('draws a cluster in the object violet', async () => {
  render(<ClusterPage clusterId={clusterDetailProducer.clusterId} />);
  expect(await weeklyBarFills()).toEqual(new Set([CLASS_COLORS.objects]));
  expect(await holderBarFills()).toEqual(new Set([CLASS_COLORS.objects]));
});
