import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { http, HttpResponse } from 'msw';
import { act, fireEvent, render, screen, waitFor, within } from '@/__tests__/utils/test-utils';
import { stubChartWidth, stubPointerEvents } from '@/__tests__/utils/chart-width';
import { server } from '@/__tests__/msw/server';
import { ckbytesAllocation, daoStatistics } from '@/__tests__/msw/fixtures/home';
import { CkbytesSection } from '@/components/home/ckbytes-section';
import { sectionsWithManyHints } from '@/__tests__/utils/hints';
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

  // C10: the two daily-change figures share the section's one "tap or hover", by the heading.
  it('says "tap or hover" once for the section, by the heading', async () => {
    const { container } = render(<CkbytesSection />);
    await waitFor(() =>
      expect(screen.getAllByRole('status', { name: /readout$/ }).length).toBeGreaterThanOrEqual(2)
    );
    expect(sectionsWithManyHints(container)).toEqual([]);
    const hint = container.querySelector('[data-hint="tap-or-hover"]')!;
    expect(hint.parentElement).toContainElement(screen.getByRole('heading', { level: 2 }));
  });

  it("prints the allocation's circulation exactly, decimals and unit dim, at its block", async () => {
    render(<CkbytesSection />);
    const total = await screen.findByTestId('ckbytes-circulating');
    expect(total).toHaveTextContent(/^49,868,783,034\.50933125 CKB/);
    expect(within(total).getByText('.50933125')).toHaveClass('text-text-dim');
    // what the figure is, quiet beside it, where "at #N" stood (the board's label)
    const label = within(total).getByTestId('ckbytes-label');
    expect(label).toHaveTextContent(/^in circulation$/);
    expect(label).toHaveClass('text-text-dim', 'text-small', 'ml-3');
    expect(total.lastElementChild).toBe(label);
    expect(screen.getByTestId('ckbytes-block')).toHaveTextContent('at #20,820,045');
    // Lane L: the block the total is exact at is a link.
    expect(
      within(screen.getByTestId('ckbytes-block')).getByRole('link', { name: '#20,820,045' })
    ).toHaveAttribute('href', '/mainnet/blocks/20820045');
    expect(screen.getByRole('link', { name: 'supply' })).toHaveAttribute(
      'href',
      '/mainnet/charts/total-supply'
    );
  });

  // Rubbing × Song print, phase 5 (H3, the Main board): `■ supply`, the block the allocation was
  // read at as the heading row's note — moved up from the figure, so the block is said once.
  it('heads the section as supply, the block it is read at as the note, said once', async () => {
    const { container } = render(<CkbytesSection />);
    await screen.findByTestId('ckbytes-circulating');
    const heading = screen.getByRole('heading', { level: 2, name: 'supply' });
    expect(heading).toHaveAttribute('id', 'home-ckbytes-heading');
    expect(screen.getByRole('region', { name: 'supply' })).toBe(container.querySelector('section'));
    expect(within(heading).getByRole('link', { name: 'supply' })).toHaveAttribute(
      'href',
      '/mainnet/charts/total-supply'
    );
    // the note: at the heading row's right end, outside the heading's name
    const note = container.querySelector('[data-prompt-note]') as HTMLElement;
    expect(note).toHaveTextContent(/^at #20,820,045$/);
    expect(note).toHaveClass('ml-auto');
    expect(note).toContainElement(screen.getByTestId('ckbytes-block'));
    expect(heading).not.toContainElement(note);
    expect(note.parentElement).toBe(heading.parentElement);
    // the heading and its note grow to fill the row, so the note reaches its right end
    expect(heading.parentElement!.parentElement).toHaveClass('flex-1', 'min-w-0');
    // said once: one link to the block in the whole section, the figure no longer carries it
    const blockLinks = within(container).getAllByRole('link', { name: '#20,820,045' });
    expect(blockLinks).toHaveLength(1);
    expect(note).toContainElement(blockLinks[0]);
    expect(screen.getByTestId('ckbytes-circulating')).not.toHaveTextContent('#');
  });

  it('names no block while the allocation loads or fails: no note, never a placeholder', async () => {
    server.use(
      http.get(`${API}/statistics/ckbytes-allocation`, () => new Promise<Response>(() => {}))
    );
    const { container, unmount } = render(<CkbytesSection />);
    screen.getByRole('heading', { level: 2, name: 'supply' });
    expect(container.querySelector('[data-prompt-note]')).toBeNull();
    expect(screen.queryByTestId('ckbytes-block')).toBeNull();
    expect(screen.queryByRole('complementary', { name: 'Note' })).toBeNull();
    unmount();
    serveAllocation({ ...ckbytesAllocation, free: '4149940517307133424' });
    const failed = render(<CkbytesSection />);
    await screen.findByTestId('ckbytes-allocation-error');
    expect(failed.container.querySelector('[data-prompt-note]')).toBeNull();
    expect(screen.getByRole('heading', { level: 2, name: 'supply' })).toBeInTheDocument();
  });

  // The Main board's composition: from lg a 7 / 3 grid, 48 px apart — the figure, the bar, the
  // legend and the change figures at the left, the note (the comment, then the formula) at the
  // right; below lg one column in the order it always had: the note after the legend.
  it('lays the body out as the board: the 7 / 3 grid from lg, the note after the legend below', async () => {
    render(<CkbytesSection />);
    await screen.findByTestId('ckbytes-circulating');
    const body = screen.getByTestId('ckbytes-body');
    expect(body).toHaveClass(
      'grid',
      'grid-cols-1',
      'lg:grid-cols-[minmax(0,7fr)_minmax(0,3fr)]',
      'lg:gap-x-12'
    );
    const [allocation, note, changes] = Array.from(body.children) as HTMLElement[];
    expect(body.children).toHaveLength(3);
    expect(allocation).toContainElement(screen.getByTestId('ckbytes-circulating'));
    expect(allocation).toContainElement(screen.getByRole('img', { name: /^Knowledge/ }));
    expect(allocation).toContainElement(screen.getByRole('button', { name: /^Nervos DAO/ }));
    expect(allocation).toHaveClass('lg:col-start-1', 'lg:row-start-1');
    expect(note).toBe(screen.getByRole('complementary', { name: 'Note' }));
    expect(note.tagName).toBe('ASIDE');
    expect(note).toHaveClass('lg:col-start-2', 'lg:row-start-1', 'lg:row-span-2');
    expect(changes).toContainElement(screen.getByTestId('home-knowledge-change'));
    expect(changes).toContainElement(screen.getByTestId('home-dao-change'));
    expect(changes).toContainElement(screen.getByTestId('home-dao'));
    expect(changes).toHaveClass('lg:col-start-1', 'lg:row-start-2');
  });

  it('writes the formula under the comment: C − burnt − S from the DAO header of the block', async () => {
    render(<CkbytesSection />);
    await screen.findByTestId('ckbytes-circulating');
    const note = screen.getByRole('complementary', { name: 'Note' });
    const comment = note.querySelector('.comment') as HTMLElement;
    const formula = screen.getByTestId('ckbytes-formula');
    expect(note.firstElementChild).toBe(comment);
    expect(comment.nextElementSibling).toBe(formula);
    expect(comment).toHaveClass('max-w-[60ch]');
    // the real minus sign, as the board writes it; the block as text, its link is the note's
    expect(formula.textContent).toBe(
      `C ${MINUS} burnt ${MINUS} S · from the DAO header of #20,820,045`
    );
    expect(formula).toHaveClass('text-text-ghost', 'text-meta', 'font-mono');
    expect(within(formula).queryByRole('link')).toBeNull();
  });

  it('draws the three served parts, which partition circulation exactly', async () => {
    render(<CkbytesSection />);
    const bar = await screen.findByRole('img', {
      name: 'Knowledge 0.30 percent, free 83.22 percent, Nervos DAO 16.48 percent',
    });
    expect(bar.querySelector('[data-segment="knowledge"]')).toHaveClass('bg-jade');
    expect(bar.querySelector('[data-segment="knowledge"]')).not.toHaveClass('min-w-[3px]');
    expect(bar.querySelector('[data-segment="dao"]')).toHaveClass('bg-gold');
    // Visual finish V11: free CKB is plain CKB held, the CKB mark's slate (CLASS_COLORS.ckb) as a
    // style on the segment and its legend swatch; no bracket hex
    expect(bar.querySelector('[data-segment="free"]')).toHaveStyle({
      backgroundColor: 'var(--c-slate-ckb)',
    });
    expect(document.querySelector('[data-legend="Free"] > span[aria-hidden="true"]')).toHaveStyle({
      backgroundColor: 'var(--c-slate-ckb)',
    });
    expect(
      document.querySelector('[data-legend="Knowledge"] > span[aria-hidden="true"]')
    ).toHaveClass('bg-jade');
    expect(bar.parentElement!.innerHTML).not.toMatch(/bg-\[#/);
    for (const [part, amount] of [
      ['knowledge', ckbytesAllocation.knowledge],
      ['free', ckbytesAllocation.free],
      ['dao', ckbytesAllocation.daoFree],
    ]) {
      const segment = bar.querySelector(`[data-segment="${part}"]`) as HTMLElement;
      expect(Number.parseFloat(segment.style.width)).toBeCloseTo(
        (Number(amount) / Number(ckbytesAllocation.circulating)) * 100,
        10
      );
    }
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

  it('uses raw allocation amounts for widths, including zero and a share below label precision', async () => {
    serveAllocation({
      ...ckbytesAllocation,
      circulating: '1000000000000000000',
      knowledge: '1',
      free: '999999999999999999',
      daoFree: '0',
    });
    render(<CkbytesSection />);
    const bar = await screen.findByRole('img', { name: /Knowledge 0\.00 percent/ });
    const knowledge = bar.querySelector('[data-segment="knowledge"]') as HTMLElement;
    const free = bar.querySelector('[data-segment="free"]') as HTMLElement;
    expect(Number.parseFloat(knowledge.style.width)).toBeGreaterThan(0);
    expect(Number.parseFloat(knowledge.style.width)).toBeLessThan(0.01);
    expect(Number.parseFloat(free.style.width)).toBeGreaterThan(99.99);
    expect(bar.querySelector('[data-segment="dao"]')).toBeNull();
    expect(bar).toHaveAccessibleName(/Nervos DAO 0\.00 percent/);
  });

  it('uses the served free part, never a remainder of its own', async () => {
    // free is served; a frontend remainder would read circulating − knowledge − DAO deposit.
    render(<CkbytesSection />);
    const free = await screen.findByRole('button', { name: /^Free/ });
    act(() => free.focus());
    expect(free).toHaveTextContent('41,499,405,173.07133425 CKB');
  });

  // Rubbing × Song print, phase 4: the Main board's supply note, in the voice, under the legend.
  it('comments on the allocation in the voice: the three parts, then the block they add up at', async () => {
    render(<CkbytesSection />);
    await screen.findByTestId('ckbytes-circulating');
    const comment = document.querySelector('.comment') as HTMLElement;
    expect(comment).not.toBeNull();
    expect(
      Array.from(comment.querySelectorAll('[data-comment-line]'), (l) => l.textContent)
    ).toEqual([
      'common knowledge is what live cells occupy.',
      'the rest is free, or in the dao.',
      // the served allocation's own block, never a figure the response does not carry
      'at #20,820,045 they add up to every ckbyte in circulation.',
    ]);
    // it sits with the allocation it describes, after the legend: the first thing of the note
    // that follows the allocation (phase 5, H3: the note is the board's aside)
    const legend = document.querySelector('[data-legend="Nervos DAO"]')!.parentElement!;
    expect(legend.nextElementSibling).toBeNull();
    const note = legend.parentElement!.nextElementSibling as HTMLElement;
    expect(note).toHaveAttribute('aria-label', 'Note');
    expect(note.firstElementChild).toBe(comment);
  });

  it('says nothing when the allocation fails: no comment on a figure that is not there', async () => {
    serveAllocation({ ...ckbytesAllocation, free: '4149940517307133424' });
    render(<CkbytesSection />);
    await screen.findByTestId('ckbytes-allocation-error');
    expect(document.querySelector('.comment')).toBeNull();
    expect(screen.queryByTestId('ckbytes-formula')).toBeNull();
    expect(screen.queryByRole('complementary', { name: 'Note' })).toBeNull();
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
    expect(within(headline).getByText('.47983386')).toHaveClass('text-text-dim', 'text-meta');
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
