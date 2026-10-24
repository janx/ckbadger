import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { http, HttpResponse } from 'msw';
import userEvent from '@testing-library/user-event';
import { fireEvent, render, screen, waitFor, within } from '@/__tests__/utils/test-utils';
import { stubChartWidth, stubPointerEvents } from '@/__tests__/utils/chart-width';
import { server } from '@/__tests__/msw/server';
import { uncleRateChart } from '@/__tests__/msw/fixtures/home';
import { NetworkSection } from '@/components/home/network-section';

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
    expect(screen.getByRole('heading', { level: 2, name: 'Network' })).toBeInTheDocument();

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

  it('shows the 24 h hourly view by default: the headline is the last full hour, the current hour hollow', async () => {
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
    expect(svg).toHaveAccessibleName(/15,710 on the last full day, the current day drawn hollow/);
  });

  it('turns a failed fetch into an error state', async () => {
    server.use(
      http.get(`${API}/charts/average-block-time`, () =>
        HttpResponse.json({ error: 'boom' }, { status: 500 })
      )
    );
    render(<NetworkSection />);
    const alert = await within(figure('home-block-time')).findByRole(
      'alert',
      {},
      { timeout: 3000 }
    );
    expect(alert).toHaveTextContent(/Failed to load/);
  });
});
