import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, within } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { ChartDetail } from '@/components/charts/chart-detail';
import { api } from '@/lib/api';

// Review of main..fix/readability (2026-11-18, logic finding 1): lane B2 cut a CKB chart's
// readouts to the summary form, so on a detail page the hover readout, the legend readout and
// "Last change" read `1,234,567` for 1,234,567.12345678 CKB, and nothing on the page carried the
// decimals of any day but the latest (only the head figure has a title). A readout is where a
// point's value is read: it states the exact value, its trailing zeros dropped, never padded,
// never cut. The head figure keeps the summary form with the exact value in its title.

vi.mock('@/lib/api', () => ({
  api: {
    getKnowledgeSizeChart: vi.fn(),
    getTotalSupplyChart: vi.fn(),
    getNetworkStats: vi.fn().mockResolvedValue({ syncStatus: { chartDataMayBeIncomplete: false } }),
    getHardforks: vi.fn().mockResolvedValue({ events: [] }),
  },
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));
vi.mock('@/components/charts/chart-small-multiple', () => ({ ChartSmallMultiple: () => null }));

describe('a CKB chart’s readouts state the exact value', () => {
  beforeEach(() => stubChartWidth(600));
  afterEach(() => vi.restoreAllMocks());

  it('reads a day, its in-plot reading and Last change exactly; the head keeps the summary', async () => {
    vi.mocked(api.getKnowledgeSizeChart).mockResolvedValue({
      title: 'Common Knowledge Size',
      yAxisLabel: 'CKB',
      y2AxisLabel: 'Utilization (%)',
      data: [
        { date: '2026-02-18', value: '1234567.12345678', value2: '13' },
        { date: '2026-02-19', value: '1236001.5', value2: '10' },
      ],
    });
    const { container } = render(<ChartDetail slug="knowledge-size" />);
    const plot = await screen.findByRole('img', { name: 'Knowledge size, CKB' });
    // The head figure: the summary form, every digit in its title.
    const head = screen.getByRole('group', { name: 'Knowledge size readings' });
    expect(head).toHaveTextContent('2026-02-191,236,001 CKB');
    expect(head.querySelector('[title="1,236,001.5 CKB"]')).not.toBeNull();
    // Last change: the exact signed delta, its trailing zeros dropped.
    const change = screen.getByText('Last change').nextElementSibling;
    expect(change).toHaveTextContent(/^\+1,434\.37654322 CKB$/);
    // A day read on the chart: the readout line and the in-plot reading, exact.
    fireEvent.keyDown(plot, { key: 'ArrowLeft' });
    expect(screen.getByRole('status', { name: 'Knowledge size readout' })).toHaveTextContent(
      '2026-02-18 · 1,234,567.12345678 CKB'
    );
    const reading = container.querySelector('[data-hover-reading]') as SVGGElement;
    expect(reading).not.toBeNull();
    expect(reading).toHaveTextContent('1,234,567.12345678 CKB');
  });

  it('keys the bands of a CKB chart with their exact values', async () => {
    vi.mocked(api.getTotalSupplyChart).mockResolvedValue({
      title: 'Total Supply',
      data: [
        {
          date: '2026-02-23',
          values: { circulating: '8131306075.44505907', nervosdao: '1234.50000000' },
        },
      ],
      series: [
        { key: 'circulating', label: 'Circulating', color: '#00c389' },
        { key: 'nervosdao', label: 'Nervos DAO', color: '#3b82f6' },
      ],
    });
    render(<ChartDetail slug="total-supply" />);
    const legend = await screen.findByRole('list', { name: 'Supply legend' });
    expect(
      within(legend)
        .getAllByRole('listitem')
        .map((item) => item.textContent)
    ).toEqual(['Circulating8,131,306,075.44505907 CKB', 'Nervos DAO1,234.5 CKB']);
  });
});
