import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { http, HttpResponse } from 'msw';
import userEvent from '@testing-library/user-event';
import { fireEvent, render, screen, waitFor, within } from '@/__tests__/utils/test-utils';
import { stubChartWidth, stubPointerEvents } from '@/__tests__/utils/chart-width';
import { server } from '@/__tests__/msw/server';
import { averageBlockTimeChart, txStats, uncleRateChart } from '@/__tests__/msw/fixtures/home';
import { NetworkSection } from '@/components/home/network-section';
import { sparkline } from '@/lib/blocks-text';
import { sectionsWithManyHints } from '@/__tests__/utils/hints';

const API = '/api/:network/v1';

function figure(testId: string) {
  return screen.getByTestId(testId);
}

describe('NetworkSection', () => {
  beforeEach(() => {
    stubChartWidth(280);
    stubPointerEvents();
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('draws block time, hash rate, uncle rate and transactions as same-size small multiples', async () => {
    render(<NetworkSection />);
    expect(
      screen.getByRole('heading', { level: 2, name: 'network --days 14' })
    ).toBeInTheDocument();

    await waitFor(() =>
      expect(screen.getByTestId('home-block-time-headline')).toHaveTextContent('9.57')
    );
    const blockTime = within(figure('home-block-time')).getByRole('img');
    expect(blockTime).toHaveAccessibleName(
      'Average block time per day, 10/10 to 10/23, between 8.66 and 11.10 seconds, 9.57 s on 10/23'
    );
    // Round ticks, not min..max thirds.
    expect(
      [...blockTime.querySelectorAll('[data-axis="y"] text')].map((t) => t.textContent)
    ).toEqual(['8', '9', '10', '11', '12']);
    expect(screen.getByRole('link', { name: 'Block time' })).toHaveAttribute(
      'href',
      '/mainnet/charts/average-block-time'
    );
  });

  // C10: four small multiples, one "tap or hover" — by the heading, not under each figure.
  it('says "tap or hover" once for its four figures, by the heading', async () => {
    const { container } = render(<NetworkSection />);
    await waitFor(() => expect(screen.getAllByRole('status', { name: /readout$/ }).length).toBe(4));
    expect(sectionsWithManyHints(container)).toEqual([]);
    const hint = container.querySelector('[data-hint="tap-or-hover"]')!;
    expect(hint).toHaveTextContent(/^tap or hover$/);
    expect(hint.parentElement).toContainElement(
      screen.getByRole('heading', { level: 2, name: 'network --days 14' })
    );
  });

  // Rubbing × Song print, phase 5 (H2, the Main board): `■ network --days 14` — the flag is the
  // window of the daily figures, dim after the command; the hint stays the row's right end, not a
  // note of the heading (it is the page's one hint, and registers in mount order).
  it('heads the section as a command on its window: network --days 14, the hint at the right', () => {
    const { container } = render(<NetworkSection />);
    const heading = screen.getByRole('heading', { level: 2, name: 'network --days 14' });
    expect(heading).toHaveClass('text-section');
    const [command, args] = Array.from(heading.children) as HTMLElement[];
    expect(command).toHaveTextContent(/^network$/);
    expect(args).toHaveTextContent(/^--days 14$/);
    expect(args).toHaveClass('text-text-dim', 'font-normal', 'normal-case');
    // the section is named by its command line
    expect(screen.getByRole('region', { name: 'network --days 14' })).toBe(
      container.querySelector('section')
    );
    const hint = container.querySelector('[data-hint="tap-or-hover"]') as HTMLElement;
    expect(heading.parentElement).toBe(hint.parentElement);
    expect(heading.nextElementSibling).toBe(hint);
    expect(container.querySelector('[data-prompt-note]')).toBeNull();
  });

  // Lane U follow-up (user report): hovering a chart made "CKBytes in circulation" below jump — the
  // readout line under the four figures was an empty box until a hover filled it. Hover never moves
  // layout: each row holds its text line idle, and a hover only changes the words in it.
  it('keeps every readout line the same shape before and after a hover', async () => {
    render(<NetworkSection />);
    await waitFor(() => expect(screen.getAllByRole('status', { name: /readout$/ }).length).toBe(4));
    const shape = () =>
      screen.getAllByRole('status', { name: /readout$/ }).map((row) => ({
        label: row.getAttribute('aria-label'),
        className: row.className,
        lines: [...row.children].map((child) => [child.tagName, child.className]),
        // the reading always has text: the words, or a no-break space holding the line
        text: (row.firstElementChild!.textContent ?? '').length > 0,
      }));
    const idle = shape();
    expect(idle.every((row) => row.text)).toBe(true);
    const svg = within(figure('home-uncle-rate')).getByRole('img');
    fireEvent.pointerMove(svg, { clientX: 268 });
    expect(screen.getByRole('status', { name: 'Uncle rate readout' })).toHaveTextContent('·');
    expect(shape()).toEqual(idle);
    fireEvent.pointerLeave(svg);
    expect(shape()).toEqual(idle);
  });

  it('uses exact daily decimals in the headline, caption, and accessible range', async () => {
    server.use(
      http.get(`${API}/charts/average-block-time`, () =>
        HttpResponse.json({
          ...averageBlockTimeChart,
          data: averageBlockTimeChart.data.map((point, index, all) =>
            index === all.length - 1 ? { ...point, value: '1.005' } : point
          ),
        })
      ),
      http.get(`${API}/charts/uncle-rate`, () =>
        HttpResponse.json({
          ...uncleRateChart,
          orphanRateTarget: { numerator: 201, denominator: 20000 },
          data: uncleRateChart.data.map((point, index, all) =>
            index === all.length - 1 ? { ...point, value: '0.01005' } : point
          ),
        })
      )
    );
    render(<NetworkSection />);
    await waitFor(() =>
      expect(screen.getByTestId('home-block-time-headline')).toHaveTextContent('1.01')
    );
    expect(within(figure('home-block-time')).getByRole('img')).toHaveAccessibleName(
      /1.01 s on 10\/23/
    );
    await waitFor(() =>
      expect(screen.getByTestId('home-uncle-rate-headline')).toHaveTextContent('1.01%')
    );
    expect(figure('home-uncle-rate')).toHaveTextContent('NC-Max target 1.01%');
    fireEvent.pointerMove(within(figure('home-uncle-rate')).getByRole('img'), { clientX: 268 });
    expect(screen.getByTestId('home-uncle-rate-readout')).toHaveTextContent('1.01%');
  });

  it('reads hash rate in H/s and only divides it into PH/s for display', async () => {
    render(<NetworkSection />);
    await waitFor(() =>
      expect(screen.getByTestId('home-hash-rate-headline')).toHaveTextContent('91.75')
    );
    const fig = figure('home-hash-rate');
    expect(fig).toHaveTextContent('daily, PH/s');
    expect(within(fig).getByRole('img')).toHaveAccessibleName(
      'Hash rate per day, 10/10 to 10/23, between 91.75 and 103.10 PH/s, 91.75 PH/s on 10/23'
    );
  });

  it('draws the uncle rate against the NC-Max target the API reports', async () => {
    render(<NetworkSection />);
    await waitFor(() =>
      expect(screen.getByTestId('home-uncle-rate-headline')).toHaveTextContent('2.64%')
    );
    const fig = figure('home-uncle-rate');
    expect(fig).toHaveTextContent('daily, NC-Max target 2.5%');
    const svg = within(fig).getByRole('img');
    expect(svg.querySelector('[data-role="reference"]')).toHaveAttribute('stroke-dasharray', '4 3');
    expect(svg).toHaveAccessibleName(
      /around the NC-Max target of 2.5 percent, 2.64 percent on 10\/23/
    );
  });

  it('takes the target from the response, never a constant', async () => {
    server.use(
      http.get(`${API}/charts/uncle-rate`, () =>
        HttpResponse.json({
          ...uncleRateChart,
          orphanRateTarget: { numerator: 1, denominator: 50 },
        })
      )
    );
    render(<NetworkSection />);
    await waitFor(() => expect(figure('home-uncle-rate')).toHaveTextContent('NC-Max target 2%'));
  });

  it('shows an error, not a chart, when the API omits the target', async () => {
    const { orphanRateTarget: _omitted, ...withoutTarget } = uncleRateChart;
    server.use(http.get(`${API}/charts/uncle-rate`, () => HttpResponse.json(withoutTarget)));
    render(<NetworkSection />);
    const alert = await within(figure('home-uncle-rate')).findByRole('alert');
    expect(alert).toHaveTextContent('the uncle-rate response has no orphanRateTarget');
    expect(within(figure('home-uncle-rate')).queryByRole('img')).not.toBeInTheDocument();
    expect(screen.queryByTestId('home-uncle-rate-headline')).not.toBeInTheDocument();
  });

  it('shows the 24 h hourly view by default: the headline is the last full hour, the current hour faint', async () => {
    render(<NetworkSection />);
    const fig = figure('home-transactions');
    const group = within(fig).getByRole('group', { name: 'Transactions range' });
    const h24 = within(group).getByRole('button', { name: '24 h' });
    const d14 = within(group).getByRole('button', { name: '14 d' });
    expect(h24).toHaveAttribute('aria-pressed', 'true');
    expect(d14).toHaveAttribute('aria-pressed', 'false');
    expect(h24).toHaveClass('h-8');

    await waitFor(() =>
      expect(screen.getByTestId('home-transactions-headline')).toHaveTextContent('731')
    );
    // The toggle names the range; the sub-label names only the bucket.
    expect(within(fig).getByText('per hour')).toBeInTheDocument();
    expect(fig).not.toHaveTextContent(/last 24/);
    const svg = within(fig).getByRole('img');
    const bars = svg.querySelectorAll('[data-bar]');
    expect(bars).toHaveLength(24);
    expect(bars[23]).toHaveAttribute('data-bar', 'inProgress');
    // Rubbing × Song print, phase 2: a sparkline of eighth blocks, one glyph per hour, from zero to
    // the busiest hour; the newest complete hour bright, the current one faint, no bars drawn.
    const glyphs = [...bars].map((b) => b.textContent).join('');
    expect(glyphs).toMatch(/^[▁▂▃▄▅▆▇█]{24}$/);
    expect(glyphs).toBe(sparkline(txStats.hourlyData.map((b) => b.value)));
    for (const b of bars) expect(b.tagName).toBe('text');
    expect(svg.querySelector('rect')).toBeNull();
    expect(bars[0]).toHaveAttribute('fill', 'var(--c-jade)');
    expect(bars[22]).toHaveAttribute('fill', 'var(--c-jade-bright)');
    expect(bars[23]).toHaveAttribute('fill', 'var(--c-text-ghost)');
    // Hovering the current hour reads its count so far in the caption (centre at x ≈ 267).
    fireEvent.pointerMove(svg, { clientX: 268 });
    expect(screen.getByTestId('home-transactions-headline')).toHaveTextContent('326');
    expect(screen.getByTestId('home-transactions-subtitle')).toHaveTextContent(
      '11:00 · in progress'
    );
    fireEvent.pointerLeave(svg);
    expect(screen.getByTestId('home-transactions-headline')).toHaveTextContent('731');
    expect(bars[22]).toHaveAttribute('data-bar', 'highlight');
    expect(fig).not.toHaveTextContent(/TXs Last 24 Hours/i);
  });

  it('switches to the 14 d daily view with the same in-progress rule', async () => {
    const user = userEvent.setup();
    render(<NetworkSection />);
    const fig = figure('home-transactions');
    await waitFor(() =>
      expect(screen.getByTestId('home-transactions-headline')).toHaveTextContent('731')
    );

    await user.click(within(fig).getByRole('button', { name: '14 d' }));
    expect(within(fig).getByRole('button', { name: '14 d' })).toHaveAttribute(
      'aria-pressed',
      'true'
    );
    expect(within(fig).getByRole('button', { name: '24 h' })).toHaveAttribute(
      'aria-pressed',
      'false'
    );
    expect(screen.getByTestId('home-transactions-headline')).toHaveTextContent('15,710');
    expect(within(fig).getByText('per day')).toBeInTheDocument();
    expect(fig).not.toHaveTextContent(/last 14/);
    const svg = within(fig).getByRole('img');
    const bars = svg.querySelectorAll('[data-bar]');
    expect(bars).toHaveLength(14);
    expect(bars[13]).toHaveAttribute('data-bar', 'inProgress');
    fireEvent.pointerMove(svg, { clientX: 264 }); // the current day, centre at x ≈ 263.5
    expect(screen.getByTestId('home-transactions-headline')).toHaveTextContent('14,122');
    expect(screen.getByTestId('home-transactions-subtitle')).toHaveTextContent(
      '10/24 · in progress'
    );
    fireEvent.pointerLeave(svg);
    expect(svg).toHaveAccessibleName(/15,710 on the last full day, the current day drawn faint/);
  });

  it('draws every in-progress bucket faint and heads with the newest complete one', async () => {
    // The first minutes of an hour: the previous hour is still in progress too.
    const hourlyData = txStats.hourlyData.map((bucket, i, all) => ({
      ...bucket,
      complete: i < all.length - 2,
    }));
    server.use(
      http.get(`${API}/statistics/tx-stats`, () => HttpResponse.json({ ...txStats, hourlyData }))
    );
    render(<NetworkSection />);
    const fig = figure('home-transactions');
    // 22 complete buckets: the newest complete one is index 21.
    await waitFor(() =>
      expect(screen.getByTestId('home-transactions-headline')).toHaveTextContent(
        hourlyData[21].value.toLocaleString('en-US')
      )
    );
    const bars = within(fig).getByRole('img').querySelectorAll('[data-bar]');
    expect(bars[21]).toHaveAttribute('data-bar', 'highlight');
    expect(bars[22]).toHaveAttribute('data-bar', 'inProgress');
    expect(bars[23]).toHaveAttribute('data-bar', 'inProgress');
    expect(within(fig).getByRole('img')).toHaveAccessibleName(
      /the 2 hours still in progress drawn faint$/
    );
    fireEvent.pointerMove(within(fig).getByRole('img'), { clientX: 258 }); // bar 22, centre ≈ 257.1
    expect(screen.getByTestId('home-transactions-subtitle')).toHaveTextContent(
      `${hourlyData[22].label} · in progress`
    );
  });

  it('shows an empty state for an empty series, which the contract allows', async () => {
    server.use(
      http.get(`${API}/statistics/tx-stats`, () =>
        HttpResponse.json({ currentHour: 0, currentDay: 0, hourlyData: [], dailyData: [] })
      )
    );
    render(<NetworkSection />);
    const fig = figure('home-transactions');
    expect(await within(fig).findByText('No transactions yet')).toBeInTheDocument();
    expect(within(fig).queryByRole('alert')).not.toBeInTheDocument();
  });

  it('fails fast when the buckets break the completeness order', async () => {
    const hourlyData = txStats.hourlyData.map((bucket, i) => ({ ...bucket, complete: i !== 3 }));
    server.use(
      http.get(`${API}/statistics/tx-stats`, () => HttpResponse.json({ ...txStats, hourlyData }))
    );
    render(<NetworkSection />);
    const alert = await within(figure('home-transactions')).findByRole('alert');
    expect(alert).toHaveTextContent(/complete bucket .* follows in-progress bucket/);
  });

  it('turns a failed fetch into an error state', async () => {
    server.use(
      http.get(`${API}/charts/average-block-time`, () =>
        HttpResponse.json({ error: 'boom' }, { status: 500 })
      )
    );
    render(<NetworkSection />);
    const alert = await within(figure('home-block-time')).findByRole('alert');
    expect(alert).toHaveTextContent(/Failed to load/);
  });
});
