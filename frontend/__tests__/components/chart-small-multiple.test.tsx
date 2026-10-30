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
    // The card headline is the design's two-decimal readout (CH-02: "91.34"); the exact value stays
    // in the title above. Like the board, the unit lives in the short subtitle ("daily, PH/s"), so
    // the headline is the bare number (visual pass: units squeezed the titles to three lines).
    const readout = screen.getByRole('group', { name: 'Hash rate current readout' });
    expect(readout).toHaveTextContent(/^900\.72$/);
    expect(readout).toHaveClass('overflow-x-auto');
    expect(screen.getByText('900.72')).toHaveClass('whitespace-nowrap');
    expect(screen.getByText('daily, PH/s')).toBeInTheDocument();
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

  // F2 charts (Charts board): a ranked card is headed by its leader's name over its share.
  it('renders top five script rows plus exact Others, headed by the leader share', async () => {
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
    const readout = screen.getByRole('group', { name: 'Scripts current readout' });
    // 7 of 28 CKB of knowledge: the leader's exact share, named above it.
    expect(readout).toHaveTextContent('25.00%');
    expect(readout.parentElement).toHaveTextContent(/^top0/);
    expect(screen.getByText('%')).toHaveClass('whitespace-nowrap');
    expect(screen.getByText('share of knowledge, top 5')).toBeInTheDocument();
  });

  it('names the leading miner in the short head…tail form on the card (CH-04)', async () => {
    const miners = CHART_BY_SLUG.get('miner-address-distribution')!;
    const address = `ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq${'x'.repeat(30)}93hhsj`;
    render(
      <ChartSmallMultiple
        definition={{
          ...miners,
          fetch: async () => ({
            title: 'Miners',
            windowDays: 7,
            totalBlocks: 10,
            fromDate: '2026-10-21',
            toDate: '2026-10-27',
            data: [
              {
                minerLockHash: `0x${'a'.repeat(64)}`,
                address,
                minerName: null,
                blocksMined: 6,
                percentage: '60.0000',
              },
            ],
          }),
        }}
      />
    );
    const list = await screen.findByRole('list', { name: 'Miners ranked preview' });
    expect(list).toHaveTextContent('ckb1qzda0c…93hhsj60.0000%');
    expect(screen.queryByText(new RegExp(address))).not.toBeInTheDocument();
    expect(list).toHaveTextContent('Others40.0000%');
    // The board's headline: the top miner's share under "top", grouped like any rate (CH-31).
    const readout = screen.getByRole('group', { name: 'Miners current readout' });
    expect(readout).toHaveTextContent('60.00%');
    expect(readout.parentElement).toHaveTextContent(/^top/);
  });

  // F2 charts group (CH-06, CH-07): classed bands carry a legend; Assets rows carry class marks.
  it('lists every band of a composition card with its class colour and share (CH-07)', async () => {
    const kind = CHART_BY_SLUG.get('common-knowledge-composition')!;
    render(
      <ChartSmallMultiple
        definition={{
          ...kind,
          fetch: async () => ({
            title: 'Composition',
            series: ['transfer', 'dao', 'udt', 'nftSpore', 'otherContracts'].map((key) => ({
              key,
              label: key,
              color: '#000',
            })),
            data: [
              {
                date: '2026-10-29',
                values: {
                  transfer: '100',
                  dao: '300',
                  udt: '50',
                  nftSpore: '40',
                  otherContracts: '10',
                },
              },
            ],
          }),
        }}
      />
    );
    const legend = await screen.findByRole('list', { name: 'Knowledge by kind legend' });
    const items = [...legend.querySelectorAll('li')];
    expect(items.map((item) => item.textContent)).toEqual([
      'Nervos DAO60.00 %',
      'Plain CKB20.00 %',
      'Tokens10.00 %',
      'Objects8.00 %',
      'Other scripts2.00 %',
    ]);
    expect(items[0].querySelector('span[aria-hidden]')).toHaveStyle({
      backgroundColor: '#f2c55c',
    });
    const readout = screen.getByRole('group', { name: 'Knowledge by kind current readout' });
    expect(readout.parentElement).toHaveTextContent(/^nervos dao/);
  });

  it('marks each Assets row with its class instead of a "(token)" suffix (CH-06)', async () => {
    const assets = CHART_BY_SLUG.get('most-utilized-assets')!;
    const share = {
      title: 'Used',
      series: [
        { key: 'top0', label: 'RGB++ (token)', color: '#000' },
        { key: 'top1', label: '拯救民主 (object)', color: '#000' },
        { key: 'others', label: 'Others', color: '#000' },
      ],
      data: [{ date: '2026-10-29', values: { top0: '300', top1: '200', others: '500' } }],
    };
    render(
      <ChartSmallMultiple
        definition={{
          ...assets,
          fetch: async () => ({ title: 'Assets', usedShare: share, capacityShare: share }),
        }}
      />
    );
    const list = await screen.findByRole('list', { name: 'Assets ranked preview' });
    const rows = [...list.querySelectorAll('li')];
    expect(rows[0]).toHaveTextContent('RGB++30.00%');
    expect(rows[0].querySelector('[data-class="tokens"]')).not.toBeNull();
    expect(rows[1]).toHaveTextContent('拯救民主20.00%');
    expect(rows[1].querySelector('[data-class="objects"]')).not.toBeNull();
    expect(list).not.toHaveTextContent('(token)');
  });
});
