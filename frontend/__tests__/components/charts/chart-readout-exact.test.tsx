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

  // Re-pinned for lane D1 (the chart-hero board, rule 2): "Last change" goes; the change is a
  // figure of the head's sentence, cut like the head (rule 2: every figure cut, never rounded),
  // and every readout of a day stays exact.
  it('reads a day and its in-plot reading exactly; the head and its sentence keep the summary', async () => {
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
    const head = screen.getByTestId('figure-value');
    expect(head).toHaveTextContent('1,236,001 CKB');
    expect(head).toHaveAttribute('title', '1,236,001.5 CKB');
    // The change, in the sentence: the exact delta (1,434.37654322) in the summary form, cut.
    expect(document.querySelector('[data-role="sentence-words"]')).toHaveTextContent(
      '1,434 CKB up on the day before.'
    );
    // A day read on the chart: the readout line and the in-plot reading, exact.
    fireEvent.keyDown(plot, { key: 'ArrowLeft' });
    expect(screen.getByRole('status', { name: 'Knowledge size readout' })).toHaveTextContent(
      '2026-02-18 · 1,234,567.12345678 CKB'
    );
    const reading = container.querySelector('[data-hover-reading]') as SVGGElement;
    expect(reading).not.toBeNull();
    expect(reading).toHaveTextContent('1,234,567.12345678 CKB');
  });

  // Re-pinned for lane D1 (rule 5): a stack's bands are named at their right ends with their
  // shares (was a key under the plot reading each band's value); a day's exact value per band is
  // read on its readout line.
  it('reads the bands of a CKB chart with their exact values', async () => {
    vi.mocked(api.getTotalSupplyChart).mockResolvedValue({
      title: 'Total Supply',
      data: [
        {
          date: '2026-02-23',
          // the three places the API always sends (the head's sentence reads all three)
          values: { circulating: '8131306075.44505907', nervosdao: '1234.50000000', burnt: '0.1' },
        },
      ],
      series: [
        { key: 'circulating', label: 'Circulating', color: '#00c389' },
        { key: 'nervosdao', label: 'Nervos DAO', color: '#3b82f6' },
        { key: 'burnt', label: 'Burnt', color: '#6b7280' },
      ],
    });
    render(<ChartDetail slug="total-supply" />);
    const plot = await screen.findByRole('img', { name: 'Supply, CKB' });
    fireEvent.keyDown(plot, { key: 'ArrowLeft' });
    expect(screen.getByRole('status', { name: 'Supply readout' })).toHaveTextContent(
      '2026-02-23 · Circulating 8,131,306,075.44505907 CKB · Nervos DAO 1,234.5 CKB · Burnt 0.1 CKB'
    );
  });
});
