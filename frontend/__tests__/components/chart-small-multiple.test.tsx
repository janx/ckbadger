import { describe, expect, it, vi } from 'vitest';
import { renderPaused } from '@/__tests__/utils/paused-read';
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
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
    // Re-pinned for lane D1 (rule 9): the figure's title is its exact value (the date stands in
    // the head's second line, was in the title)
    expect(screen.getByTitle('900.7199254740993 PH/s')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Hash rate' })).toHaveAttribute(
      'href',
      '/mainnet/charts/hash-rate'
    );
    // The card headline is the design's two-decimal readout (CH-02: "91.34"); the exact value stays
    // in the title above. Like the board, the unit lives in the short subtitle ("daily, PH/s"), so
    // the headline is the bare number (visual pass: units squeezed the titles to three lines).
    // Re-pinned for CH-14 (UI review 2026-11-14): every card figure carries its unit, bound to it
    // by a no-break space (was the bare number, the unit only in the subtitle).
    // Re-pinned for lane D1: the page's own head figure, cut, never rounded (900.7199… is 900.71,
    // was 900.72); the head's line is never cut, so it scrolls no more (was overflow-x-auto).
    const readout = screen.getByRole('group', { name: 'Hash rate current readout' });
    expect(readout.textContent).toBe('900.71\u00a0PH/s');
    expect(readout).toHaveClass('whitespace-nowrap');
    expect(screen.getByText('daily estimate')).toBeInTheDocument();
    expect(screen.getByText('2024-01-01')).toBeInTheDocument();
  });

  // Responsive review 2026-11-01: in a four-column "Related charts" grid at 1280 (and two columns
  // at 640) the readout block could not shrink and its date was cut at the card's edge. Re-pinned
  // for lane D1 (the chart-hero board, rule 9, CH-16): the head no longer wraps (a readout that
  // dropped under its title put the plots of one row on different lines); it is two fixed lines,
  // each written to fit the narrowest card (chart-card-head.test.tsx), the figure at the right end.
  it('keeps the head on two fixed lines, the figure at the right end, never cut', async () => {
    render(
      <ChartSmallMultiple definition={{ ...base, fetch: vi.fn().mockResolvedValue(result) }} />
    );
    const readoutBlock = (await screen.findByTitle('900.7199254740993 PH/s')) as HTMLElement;
    const header = readoutBlock.parentElement as HTMLElement;
    expect(header).not.toHaveClass('flex-wrap');
    expect(readoutBlock).toHaveClass('ml-auto', 'whitespace-nowrap');
    expect(readoutBlock).not.toHaveClass('shrink-0');
    expect(screen.getByText('2024-01-01')).toHaveClass('whitespace-nowrap');
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
        color: 'var(--c-jade)',
      })),
      data: [
        {
          date: '2024-01-01',
          values: {
            ...Object.fromEntries(keys.map((key, i) => [key, String((7 - i) * 100_000_000)])),
            // lane D1: the API's rest band, rank six and its remainder (2 + 1 CKB)
            rest: '300000000',
            // the day's whole, the route's (the D1 hand-back, item 2): 7 + 6 + … + 1 CKB
            total: '2800000000',
          },
        },
      ],
      bands: [
        ...keys.slice(0, 5).map((key) => ({ key, label: key, members: [key] })),
        { key: 'rest', label: 'Others', members: ['top5', 'others'] },
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
    // Re-pinned for lane D1 (rule 9, CH-07): the card heads with the page's own figure, the
    // largest script's CKB (was its share, 25.00%, in a headline of the card's own); the shares
    // stay on the rows.
    expect(screen.getByTitle('7 CKB')).toBeInTheDocument();
    const readout = screen.getByRole('group', { name: 'Scripts current readout' });
    expect(readout.textContent).toBe('7\u00a0CKB');
    expect(list).toHaveTextContent('top025.00%');
    expect(screen.getByText('largest script')).toBeInTheDocument();
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
    // Two decimals, the one share form (re-pinned for CH-03 / CH-14: was the API's 60.0000%).
    // R9 (re-pinned): the short address is `ckb1…` and its last 8 characters.
    expect(list).toHaveTextContent('ckb1…xx93hhsj60.00%');
    expect(screen.queryByText(new RegExp(address))).not.toBeInTheDocument();
    expect(list).toHaveTextContent('Others40.00%');
    // Lane L: the miner's short address links to its address page; "Others" is no entity.
    const miner = within(list).getByRole('link', { name: 'ckb1…xx93hhsj' });
    expect(miner).toHaveAttribute('href', `/mainnet/address/${address}`);
    expect(miner).toHaveAttribute('title', address);
    expect(within(list).queryByRole('link', { name: 'Others' })).toBeNull();
    // Re-pinned for lane D1 (rule 9, CH-07): the page's own figure, the largest miner's blocks
    // under its title (was its share under "top"); the window is the head's date.
    const readout = screen.getByRole('group', { name: 'Miners current readout' });
    expect(readout.textContent).toBe('6');
    expect(screen.getByText('largest miner')).toBeInTheDocument();
    expect(screen.getByText('2026-10-21 → 2026-10-27')).toBeInTheDocument();
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
                  // the day's knowledge size, the route's (the D1 hand-back, item 2)
                  knowledgeSize: '500',
                },
              },
            ],
          }),
        }}
      />
    );
    const legend = await screen.findByRole('list', { name: 'Knowledge by kind legend' });
    const items = [...legend.querySelectorAll('li')];
    // Re-pinned for CH-14 (UI review 2026-11-14): one percent form, no space before the sign.
    expect(items.map((item) => item.textContent)).toEqual([
      'Nervos DAO60.00%',
      'Plain CKB20.00%',
      'Tokens10.00%',
      'Objects8.00%',
      'Other scripts2.00%',
    ]);
    expect(items[0].querySelector('span[aria-hidden]')).toHaveStyle({
      backgroundColor: 'var(--c-gold)',
    });
    // Re-pinned for lane D1 (rule 9, CH-07): the page's figure, the largest kind's CKB, its
    // caption the page's words (was its share under "nervos dao")
    const readout = screen.getByRole('group', { name: 'Knowledge by kind current readout' });
    expect(readout.textContent).toBe('300\u00a0CKB');
    expect(screen.getByText('largest kind')).toBeInTheDocument();
  });

  // Lane L: two assets that share a name are told apart by a short id; the link to each keeps
  // the full id in its hover title.
  it('links a same-named asset by its page, the full id in the title (lane L)', async () => {
    const assets = CHART_BY_SLUG.get('most-utilized-assets')!;
    const a = `0x5cb3322a${'00'.repeat(23)}d76ab4fa`;
    const b = `0x7d4e1f0b${'11'.repeat(23)}aa01cc02`;
    const share = {
      title: 'Used',
      series: [
        { key: 'top0', label: 'MEMES (token)', color: '#000', id: `token:${a}` },
        { key: 'top1', label: 'MEMES (token)', color: '#000', id: `token:${b}` },
        { key: 'others', label: 'Others', color: '#000' },
      ],
      // the day's whole, the route's (the D1 hand-back, item 2)
      data: [
        { date: '2026-10-29', values: { top0: '300', top1: '200', others: '500', total: '1000' } },
      ],
      // lane D1: two named ranks, each its own band
      bands: ['top0', 'top1', 'others'].map((key) => ({ key, label: key, members: [key] })),
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
    const link = within(list).getByRole('link', { name: 'MEMES · 0x5cb3…b4fa' });
    expect(link).toHaveAttribute('href', `/mainnet/tokens/${a}`);
    expect(link.getAttribute('title')).toContain(a);
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
      // the day's whole, the route's (the D1 hand-back, item 2)
      data: [
        { date: '2026-10-29', values: { top0: '300', top1: '200', others: '500', total: '1000' } },
      ],
      // lane D1: two named ranks, each its own band
      bands: ['top0', 'top1', 'others'].map((key) => ({ key, label: key, members: [key] })),
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

// A1 review fix, round 2: a read paused offline, or in a hidden tab between retries, is still
// awaited; the section says it is loading, never "no …" or "failed", and resumes with the data.
describe('ChartSmallMultiple while its read is paused', () => {
  it('keeps its loading form, not nothing, then draws once back online', async () => {
    const fetch = vi.fn(async () => result);
    const { resume } = renderPaused(<ChartSmallMultiple definition={{ ...base, fetch }} />);
    expect(await screen.findByLabelText('Hash rate loading')).toBeInTheDocument();
    expect(fetch).not.toHaveBeenCalled();
    resume();
    expect(await screen.findByRole('img', { name: 'Hash rate, PH/s' })).toBeInTheDocument();
    expect(screen.queryByLabelText('Hash rate loading')).toBeNull();
  });
});
