import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { ChartDetail } from '@/components/charts/chart-detail';
import { api } from '@/lib/api';

// UI review 2026-11-14, CH-04: on Scripts and Assets the "Owned live capacity" history chart was
// captioned "Share of common knowledge, top scripts · 2555 returned dates": the page subtitle was
// reused as every history chart's caption, the count had no thousands separator, and "returned
// dates" is the API's word.

vi.mock('@/lib/api', () => ({
  api: {
    getMostUtilizedScriptsChart: vi.fn(),
    getMostUtilizedAssetsChart: vi.fn(),
    getNetworkStats: vi.fn(),
    getHardforks: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));
vi.mock('@/components/charts/chart-small-multiple', () => ({ ChartSmallMultiple: () => null }));
const chartProps = vi.hoisted(() => [] as Record<string, unknown>[]);
vi.mock('@/components/ui/chart', () => ({
  Chart: (props: { title: string }) => {
    chartProps.push(props);
    return <div role="img" aria-label={props.title} />;
  },
}));
const lastChart = (title: string) => chartProps.filter((props) => props.title === title).at(-1)!;

const DAYS = 1234;
const dates = Array.from({ length: DAYS }, (_, i) =>
  new Date(Date.UTC(2020, 0, 1 + i)).toISOString().slice(0, 10)
);
const share = (label: string) => ({
  title: 'Share',
  series: [
    { key: 'top0', label, color: '#000' },
    { key: 'others', label: 'Others', color: '#000' },
  ],
  data: dates.map((date) => ({ date, values: { top0: '5900000000000000', others: '100000000' } })),
});

describe('history chart captions (CH-04)', () => {
  beforeEach(() => {
    chartProps.length = 0;
    vi.mocked(api.getNetworkStats).mockResolvedValue({
      syncStatus: { chartDataMayBeIncomplete: false },
    } as Awaited<ReturnType<typeof api.getNetworkStats>>);
    vi.mocked(api.getHardforks).mockResolvedValue({ events: [] } as unknown as Awaited<
      ReturnType<typeof api.getHardforks>
    >);
    vi.mocked(api.getMostUtilizedScriptsChart).mockResolvedValue({
      title: 'Scripts',
      usedShare: share('Default Lock'),
      capacityShare: share('Default Lock'),
    });
    vi.mocked(api.getMostUtilizedAssetsChart).mockResolvedValue({
      title: 'Assets',
      usedShare: share('RGB++ (token)'),
      capacityShare: share('RGB++ (token)'),
    });
  });

  it('captions each Scripts history chart with its own quantity, unit and span', async () => {
    render(<ChartDetail slug="most-utilized-scripts" />);
    await screen.findByRole('img', { name: 'Owned live capacity' });
    expect(lastChart('Occupied common knowledge').subtitle).toBe(
      'CKB occupied, by script · 1,234 days'
    );
    expect(lastChart('Owned live capacity').subtitle).toBe('CKB owned, by script · 1,234 days');
    expect(screen.getByRole('heading', { name: 'Owned live capacity 1,234 days' })).toBeTruthy();
  });

  it('captions the Assets history charts by asset', async () => {
    render(<ChartDetail slug="most-utilized-assets" />);
    await screen.findByRole('img', { name: 'Owned live capacity' });
    expect(lastChart('Occupied common knowledge').subtitle).toBe(
      'CKB occupied, by asset · 1,234 days'
    );
    expect(lastChart('Owned live capacity').subtitle).toBe('CKB owned, by asset · 1,234 days');
  });

  it('keeps the API words out of tier 1', async () => {
    render(<ChartDetail slug="most-utilized-scripts" />);
    await screen.findByRole('img', { name: 'Owned live capacity' });
    const tier2 = document.querySelector('[data-tier="2"]')!;
    const tier1Text = document.body.textContent!.replace(tier2.textContent!, '');
    expect(tier1Text).not.toMatch(/returned dates|owned-capacity dates/);
    for (const props of chartProps)
      expect(String(props.subtitle)).not.toMatch(/returned dates|owned-capacity dates/);
  });
});
