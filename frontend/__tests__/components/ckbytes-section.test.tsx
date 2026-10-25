import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { http, HttpResponse } from 'msw';
import { act, fireEvent, render, screen, waitFor, within } from '@/__tests__/utils/test-utils';
import { stubChartWidth, stubPointerEvents } from '@/__tests__/utils/chart-width';
import { server } from '@/__tests__/msw/server';
import { ckbytesAllocation, daoStatistics } from '@/__tests__/msw/fixtures/home';
import { CkbytesSection } from '@/components/home/ckbytes-section';
import { MINUS } from '@/lib/chart-ticks';
import type { CkbytesAllocation } from '@/lib/api';

const API = '/api/:network/v1';

function serveAllocation(allocation: CkbytesAllocation) {
  server.use(http.get(`${API}/statistics/ckbytes-allocation`, () => HttpResponse.json(allocation)));
}

describe('CkbytesSection', () => {
  beforeEach(() => {
    stubChartWidth(280);
    stubPointerEvents();
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("prints the allocation's circulation exactly, decimals and unit dim, at its block", async () => {
    render(<CkbytesSection />);
    const total = await screen.findByTestId('ckbytes-circulating');
    expect(total).toHaveTextContent('49,868,783,034.50933125 CKB');
    expect(within(total).getByText('.50933125')).toHaveClass('text-text-dim');
    expect(screen.getByTestId('ckbytes-block')).toHaveTextContent('at #20,820,045');
    expect(screen.getByRole('link', { name: 'CKBytes in circulation' })).toHaveAttribute(
      'href',
      '/mainnet/charts/total-supply'
    );
  });

  it('draws the three served parts, which partition circulation exactly', async () => {
    render(<CkbytesSection />);
    const bar = await screen.findByRole('img', {
      name: 'Knowledge 0.30 percent, free 83.22 percent, Nervos DAO 16.48 percent',
    });
    expect(bar.querySelector('[data-segment="knowledge"]')).toHaveClass('bg-jade', 'min-w-[3px]');
    expect(bar.querySelector('[data-segment="dao"]')).toHaveClass('bg-gold');
    expect(document.querySelector('[data-legend="Knowledge"]')).toHaveTextContent(
      'Knowledge151.13M CKB151.1 MB · 0.30%'
    );
    expect(document.querySelector('[data-legend="Free"]')).toHaveTextContent(
      'Free41.50B CKB83.22%'
    );
    // The DAO part is the DAO cells' free capacity; their whole capacity is context only.
    expect(document.querySelector('[data-legend="Nervos DAO"]')).toHaveTextContent(
      'Nervos DAO8.22B CKB16.48% · 8.22B locked'
    );
  });

  it('uses the served free part, never a remainder of its own', async () => {
    // free is served; a frontend remainder would read circulating − knowledge − DAO deposit.
    render(<CkbytesSection />);
    const free = await screen.findByRole('button', { name: /^Free/ });
    act(() => free.focus());
    expect(free).toHaveTextContent('41,499,405,173.07133425 CKB');
  });

  it('fails fast when the served parts do not add up to the circulation', async () => {
    serveAllocation({ ...ckbytesAllocation, free: '4149940517307133424' });
    render(<CkbytesSection />);
    const error = await screen.findByTestId('ckbytes-allocation-error');
    expect(error).toHaveAttribute('role', 'alert');
    expect(error).toHaveTextContent(
      'The allocation at #20,820,045 does not partition circulation: 4,986,878,303,450,933,125 ≠ knowledge 15,113,424,500,000,000 + DAO free 821,824,361,643,799,700 + free 4,149,940,517,307,133,424 shannons.'
    );
    expect(screen.queryByTestId('ckbytes-circulating')).not.toBeInTheDocument();
  });

  it('fails fast on a negative part', async () => {
    serveAllocation({
      ...ckbytesAllocation,
      free: '-1',
      knowledge: String(
        BigInt(ckbytesAllocation.knowledge) + BigInt(ckbytesAllocation.free) + BigInt(1)
      ),
    });
    render(<CkbytesSection />);
    expect(await screen.findByTestId('ckbytes-allocation-error')).toHaveTextContent(/free -1/);
  });

  it('shows the failed endpoint as an error, not a stale or invented bar', async () => {
    server.use(
      http.get(`${API}/statistics/ckbytes-allocation`, () =>
        HttpResponse.json({ error: 'DAO aggregate row has the old format' }, { status: 500 })
      )
    );
    render(<CkbytesSection />);
    expect(await screen.findByTestId('ckbytes-allocation-error')).toHaveTextContent(
      /Failed to load/
    );
    expect(screen.queryByRole('img', { name: /^Knowledge/ })).not.toBeInTheDocument();
  });

  it('charts the last 30 days of knowledge-size change, clipping the outlier', async () => {
    render(<CkbytesSection />);
    const fig = screen.getByTestId('home-knowledge-change');
    await waitFor(() =>
      expect(screen.getByTestId('home-knowledge-change-headline')).toHaveTextContent('+82,991')
    );
    expect(fig).toHaveTextContent('daily change, CKB');
    const svg = within(fig).getByRole('img');
    expect(svg.querySelectorAll('[data-bar]')).toHaveLength(30);
    expect([...svg.querySelectorAll('[data-axis="y"] text')].map((t) => t.textContent)).toEqual([
      '2M',
      '0',
      `${MINUS}2M`,
    ]);
    const clipped = svg.querySelectorAll('[data-clipped="true"]');
    expect(clipped).toHaveLength(1);
    expect(clipped[0]).toHaveTextContent(`${MINUS}8.94M`);
    expect(svg).toHaveAccessibleName(/drawn clipped: −8\.94M on 09\/27/);
  });

  it('charts DAO deposit changes exactly: the headline is the last day, truncated to whole CKB', async () => {
    render(<CkbytesSection />);
    const fig = screen.getByTestId('home-dao-change');
    await waitFor(() =>
      expect(screen.getByTestId('home-dao-change-headline')).toHaveTextContent('+168,366')
    );
    const svg = within(fig).getByRole('img');
    // Hovering the last bar (centre at x ≈ 268) reads its exact change, decimals dim.
    fireEvent.pointerMove(svg, { clientX: 268 });
    const headline = screen.getByTestId('home-dao-change-headline');
    expect(headline).toHaveTextContent('+168,366.47983386');
    expect(within(headline).getByText('.47983386')).toHaveClass('text-text-dim', 'text-[10px]');
    expect(screen.getByTestId('home-dao-change-subtitle')).toHaveTextContent('10/23');
    fireEvent.pointerLeave(svg);
    expect(headline).toHaveTextContent(/^\+168,366$/);
    expect(screen.getByTestId('home-dao-change-subtitle')).toHaveTextContent('daily change, CKB');
    expect(svg.querySelector('[data-clipped="true"]')).toHaveTextContent('+28.12M');
  });

  it('shows the DAO block: exact deposit, APC, depositors, active deposits and the signed 24 h change', async () => {
    render(<CkbytesSection />);
    const dao = screen.getByTestId('home-dao');
    const deposited = await within(dao).findByTestId('home-dao-deposited');
    expect(deposited).toHaveTextContent('8,128,895,616.43799700 CKB');
    const terms = [...dao.querySelectorAll('dt')].map((t) => t.textContent);
    expect(terms).toEqual(['Estimated APC', 'Depositors', 'Active deposits', 'Change in 24 h']);
    expect(dao).toHaveTextContent('1.99%');
    expect(dao).toHaveTextContent(`16,673 ${MINUS}3 in 24 h`);
    expect(dao).toHaveTextContent('22,379');
    const change = within(dao).getByTestId('home-dao-change-24h');
    expect(change).toHaveTextContent(`${MINUS}2,578,825 CKB`);
    expect(change).toHaveClass('text-negative');
    expect(change).toHaveAttribute('title', `${MINUS}2,578,825.48689593 CKB`);
    expect(dao).not.toHaveTextContent(/Avg deposit/i);
    expect(within(dao).getByRole('link', { name: 'DAO details →' })).toHaveAttribute(
      'href',
      '/mainnet/dao'
    );
  });

  it('shows a dash, not a zero, when there is no previous day to compare with', async () => {
    const first = { ...daoStatistics, depositChange24h: null, depositorsChange24h: null };
    server.use(http.get(`${API}/dao/statistics`, () => HttpResponse.json(first)));
    render(<CkbytesSection />);
    const dao = screen.getByTestId('home-dao');
    await within(dao).findByTestId('home-dao-deposited');
    expect(within(dao).queryByTestId('home-dao-change-24h')).not.toBeInTheDocument();
    expect(dao).toHaveTextContent('Change in 24 h—');
    // No depositor change either.
    expect(dao).toHaveTextContent('Depositors16,673Active deposits');
  });
});

describe('CkbytesSection allocation highlight', () => {
  beforeEach(() => {
    stubChartWidth(280);
    stubPointerEvents();
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  async function renderBar() {
    render(<CkbytesSection />);
    const bar = await screen.findByRole('img', { name: /^Knowledge 0\.30 percent/ });
    const seg = (part: string) => bar.querySelector(`[data-segment="${part}"]`) as HTMLElement;
    return { bar, seg };
  }
  const legend = (name: RegExp) => screen.getByRole('button', { name });

  function expectHighlighted(seg: (p: string) => HTMLElement, on: string) {
    for (const part of ['knowledge', 'free', 'dao']) {
      if (part === on) {
        expect(seg(part)).toHaveAttribute('data-active', 'true');
        expect(seg(part)).not.toHaveClass('opacity-35');
      } else {
        expect(seg(part)).not.toHaveAttribute('data-active');
        expect(seg(part)).toHaveClass('opacity-35');
      }
    }
  }
  function expectNoHighlight(seg: (p: string) => HTMLElement) {
    for (const part of ['knowledge', 'free', 'dao']) {
      expect(seg(part)).not.toHaveAttribute('data-active');
      expect(seg(part)).not.toHaveClass('opacity-35');
    }
  }

  it('highlights a hovered segment and reads it out exactly in its legend item', async () => {
    const { seg } = await renderBar();
    fireEvent.pointerEnter(seg('free'));
    expectHighlighted(seg, 'free');
    const free = legend(/^Free/);
    expect(free).toHaveTextContent('Free41,499,405,173.07133425 CKB83.22%');
    expect(within(free).getByText('Free')).toHaveClass('text-text-bright');

    fireEvent.pointerLeave(seg('free'));
    expectNoHighlight(seg);
    expect(free).toHaveTextContent('Free41.50B CKB83.22%');
    expect(within(free).getByText('Free')).toHaveClass('text-text');
  });

  it('highlights the segment of a hovered legend item', async () => {
    const { seg } = await renderBar();
    const dao = legend(/^Nervos DAO/);
    fireEvent.pointerEnter(dao);
    expectHighlighted(seg, 'dao');
    expect(dao).toHaveTextContent('Nervos DAO8,218,243,616.43799700 CKB16.48% · 8.22B locked');
    fireEvent.pointerLeave(dao);
    expectNoHighlight(seg);
  });

  it('highlights on keyboard focus of a legend item', async () => {
    const { seg } = await renderBar();
    const knowledge = legend(/^Knowledge/);
    act(() => knowledge.focus());
    expect(knowledge).toHaveFocus();
    expectHighlighted(seg, 'knowledge');
    expect(knowledge).toHaveTextContent('Knowledge151,134,245.00000000 CKB151.1 MB · 0.30%');
    act(() => knowledge.blur());
    expectNoHighlight(seg);
  });

  it('toggles the highlight with a tap on touch', async () => {
    const { seg } = await renderBar();
    const free = legend(/^Free/);
    const tap = (el: HTMLElement) => {
      fireEvent.pointerDown(el, { pointerType: 'touch' });
      fireEvent.pointerEnter(el, { pointerType: 'touch' });
      fireEvent.click(el);
      fireEvent.pointerLeave(el, { pointerType: 'touch' });
    };
    tap(free);
    expectHighlighted(seg, 'free');
    tap(free);
    expectNoHighlight(seg);
    tap(seg('dao'));
    expectHighlighted(seg, 'dao');
  });

  it('does not pin on a mouse click', async () => {
    const { seg } = await renderBar();
    const free = legend(/^Free/);
    fireEvent.pointerDown(free, { pointerType: 'mouse' });
    fireEvent.pointerEnter(free, { pointerType: 'mouse' });
    fireEvent.click(free);
    fireEvent.pointerLeave(free, { pointerType: 'mouse' });
    expectNoHighlight(seg);
  });
});
