import { describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import { ChartSmallMultiple } from '@/components/charts/chart-small-multiple';
import { CHART_BY_SLUG } from '@/components/charts/chart-catalogue';

vi.mock('@/components/ui/chart', () => ({
  Chart: ({ title, unit }: { title: string; unit: string }) => (
    <div role="img" aria-label={`${title}, ${unit}`} />
  ),
}));

const base = CHART_BY_SLUG.get('hash-rate')!;
const result = {
  title: 'Hash rate',
  yAxisLabel: 'H/s',
  data: [{ date: '2024-01-01', value: '900719925474099300' }],
};

describe('ChartSmallMultiple', () => {
  it('shows loading, then exact latest readout and a route link', async () => {
    let complete!: (value: typeof result) => void;
    const fetch = vi.fn(
      () =>
        new Promise<typeof result>((resolve) => {
          complete = resolve;
        })
    );
    render(<ChartSmallMultiple definition={{ ...base, fetch }} />);
    expect(screen.getByLabelText('Hash rate loading')).toBeInTheDocument();
    complete(result);
    expect(await screen.findByRole('img', { name: 'Hash rate, PH/s' })).toBeInTheDocument();
    expect(screen.getByTitle('2024-01-01: 900.7199254740993 PH/s')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Hash rate' })).toHaveAttribute(
      'href',
      '/mainnet/charts/hash-rate'
    );
    const readout = screen.getByRole('group', { name: 'Hash rate current readout' });
    expect(readout).toHaveTextContent('900.7199254740993PH/s');
    expect(readout).toHaveClass('overflow-x-auto');
    expect(screen.getByText('900.7199254740993')).toHaveClass('whitespace-nowrap');
    expect(screen.getByText('PH/s')).toHaveClass('whitespace-nowrap');
  });

  it('distinguishes an empty response from an error and permits retry', async () => {
    const fetch = vi
      .fn()
      .mockRejectedValueOnce(new Error('warmup_pending: asset cache'))
      .mockResolvedValueOnce({ ...result, data: [] });
    render(<ChartSmallMultiple definition={{ ...base, fetch }} />);
    expect(await screen.findByRole('alert')).toHaveTextContent('warmup_pending: asset cache');
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    await waitFor(() =>
      expect(screen.getByText('No indexed observations for this chart.')).toBeInTheDocument()
    );
    expect(fetch).toHaveBeenCalledTimes(2);
  });

  it('renders top five script rows plus exact Others with a CKB-valued headline', async () => {
    const ranked = CHART_BY_SLUG.get('most-utilized-scripts')!;
    const keys = ['top0', 'top1', 'top2', 'top3', 'top4', 'top5', 'others'];
    const share = {
      title: 'Used',
      series: keys.map((key) => ({
        key,
        label: key === 'others' ? 'Others' : key,
        color: '#2edba3',
      })),
      data: [
        {
          date: '2024-01-01',
          values: Object.fromEntries(keys.map((key, i) => [key, String((7 - i) * 100_000_000)])),
        },
      ],
    };
    render(
      <ChartSmallMultiple
        definition={{
          ...ranked,
          fetch: async () => ({ title: 'Scripts', usedShare: share, capacityShare: share }),
        }}
      />
    );
    const list = await screen.findByRole('list', { name: 'Scripts ranked preview' });
    expect(list.querySelectorAll('li')).toHaveLength(6);
    expect(list).toHaveTextContent('Others');
    expect(screen.getByTitle('2024-01-01: 7 CKB')).toBeInTheDocument();
    expect(screen.getByText('Largest script: top0')).toBeInTheDocument();
    const readout = screen.getByRole('group', { name: 'Scripts current readout' });
    expect(readout).toHaveTextContent('7.00CKB');
    expect(screen.getByText('CKB')).toHaveClass('whitespace-nowrap');
  });
});
