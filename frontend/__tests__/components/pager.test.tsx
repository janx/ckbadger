import { afterEach, describe, expect, it, vi } from 'vitest';
import userEvent from '@testing-library/user-event';
import { render, screen } from '@testing-library/react';
import { Pager } from '@/components/ui/pager';
import { inkOf, QUIET, READING } from '@/__tests__/utils/ink-role';
import { inFlowAt, targetHeightPx } from '@/__tests__/utils/tailwind-box';

/**
 * The pager of the controls board (gate D0′): words, not boxes, 13 px; where you are in the
 * reading ink, a way to go celadon, an end that cannot be taken quiet and not a control; nothing
 * for one page. Two modes behind one face — pages (`41–60 of 727 · 20 a page`, `← prev`,
 * `page 3 of 37`, `next →`) and a cursor in time (`← newer`, `older →`) — and the short list's own
 * forms (`all 8 branch switches ↓`, `next 50 ↓`). Every figure is the caller's, grouped.
 */
const text = (el: Element | null) => el?.textContent?.replace(/ /g, ' ') ?? null;

afterEach(() => vi.restoreAllMocks());

// Re-pinned on purpose (lane c2-finish, item 1): the pages mode takes a ranked page as
// lib/pagination.ts's rankedPageAt gives it — the page, its size, its rows (none past the end),
// the response's "more" and, where the response counts the rows, the count and the pages — where
// it took a flat from / to / total / pages that a list with no count or a shrunk list could not
// give. Every case below is the case it was, in that shape, and three are new.
describe('Pager: pages', () => {
  const page3 = {
    page: 3,
    pageSize: 20,
    hasMore: true,
    rows: { from: 41, to: 60 },
    count: { total: 727, pages: 37 },
  };
  const go = { onPrevious: () => {}, onNext: () => {} };

  it('says where the list is and goes either way: the board’s line', async () => {
    const user = userEvent.setup();
    const onPrevious = vi.fn();
    const onNext = vi.fn();
    render(
      <Pager mode="pages" label="tokens" {...page3} onPrevious={onPrevious} onNext={onNext} />
    );
    const nav = screen.getByRole('navigation', { name: 'tokens' });
    expect(text(nav)).toBe('41–60 of 727 · 20 a page← prevpage 3 of 37next →');
    await user.click(screen.getByRole('button', { name: '← prev' }));
    await user.click(screen.getByRole('button', { name: 'next →' }));
    expect(onPrevious).toHaveBeenCalledTimes(1);
    expect(onNext).toHaveBeenCalledTimes(1);
  });

  it('writes the count quiet, where it is in the reading ink and a way to go celadon, no box', () => {
    render(<Pager mode="pages" label="tokens" {...page3} {...go} />);
    expect(inkOf(screen.getByText(/^41–60 of 727/))).toBe(QUIET);
    expect(inkOf(screen.getByText('page 3 of 37'))).toBe(READING);
    for (const b of screen.getAllByRole('button')) {
      expect(b).toHaveClass('text-jade', 'text-small');
      expect(b.className).not.toMatch(/\bborder\b|\bbg-/);
    }
  });

  it('draws an end that cannot be taken in the quiet ink, and not as a control', () => {
    const { unmount } = render(
      <Pager mode="pages" label="tokens" {...page3} page={1} rows={{ from: 1, to: 20 }} {...go} />
    );
    expect(screen.queryByRole('button', { name: '← prev' })).toBeNull();
    expect(inkOf(screen.getByText('← prev'))).toBe(QUIET);
    expect(screen.getByRole('button', { name: 'next →' })).toBeInTheDocument();
    unmount();
    render(
      <Pager
        mode="pages"
        label="tokens"
        {...page3}
        page={37}
        rows={{ from: 721, to: 727 }}
        hasMore={false}
        {...go}
      />
    );
    expect(screen.queryByRole('button', { name: 'next →' })).toBeNull();
    expect(inkOf(screen.getByText('next →'))).toBe(QUIET);
  });

  it('draws nothing for one page', () => {
    const { container } = render(
      <Pager
        mode="pages"
        label="tokens"
        page={1}
        pageSize={20}
        hasMore={false}
        rows={{ from: 1, to: 8 }}
        count={{ total: 8, pages: 1 }}
        {...go}
      />
    );
    expect(container).toBeEmptyDOMElement();
  });

  it('groups every figure and computes none: it writes the pages it is given', () => {
    render(
      <Pager
        mode="pages"
        label="blocks"
        page={100001}
        pageSize={20}
        hasMore
        rows={{ from: 2000001, to: 2000020 }}
        // the caller's figure, not ⌈53,025,887 / 20⌉ = 2,651,295: the pager divides nothing
        count={{ total: 53025887, pages: 2651300 }}
        {...go}
      />
    );
    expect(screen.getByText('page 100,001 of 2,651,300')).toBeInTheDocument();
    expect(text(screen.getByText(/^2,000,001–2,000,020 of 53,025,887/))).toBe(
      '2,000,001–2,000,020 of 53,025,887 · 20 a page'
    );
  });

  it('keeps “· 20 a page” from 640 and leaves it below, the phone’s line', () => {
    render(<Pager mode="pages" label="tokens" {...page3} {...go} />);
    const perPage = screen.getByText(/· 20 a page/);
    expect(inFlowAt(perPage, 640)).toBe(true);
    expect(inFlowAt(perPage, 390)).toBe(false);
  });

  it('gives a way to go a 28 px target from 640 and 32 below', () => {
    render(<Pager mode="pages" label="tokens" {...page3} {...go} />);
    for (const b of screen.getAllByRole('button'))
      expect([1280, 640, 390, 360].map((w) => targetHeightPx(b, w))).toEqual([28, 28, 32, 32]);
  });

  // Item 1a: an ordered list whose response counts no rows (the gallery's search): its rows and
  // its size, the page alone, the next by the response's own "more".
  it('pages an ordered list with no count: its rows, the page alone, the next by “more”', async () => {
    const user = userEvent.setup();
    const onNext = vi.fn();
    const { unmount } = render(
      <Pager
        mode="pages"
        label="objects"
        page={2}
        pageSize={24}
        hasMore
        rows={{ from: 25, to: 48 }}
        count={null}
        onPrevious={() => {}}
        onNext={onNext}
      />
    );
    expect(text(screen.getByRole('navigation', { name: 'objects' }))).toBe(
      '25–48 · 24 a page← prevpage 2next →'
    );
    await user.click(screen.getByRole('button', { name: 'next →' }));
    expect(onNext).toHaveBeenCalledTimes(1);
    unmount();
    render(
      <Pager
        mode="pages"
        label="objects"
        page={3}
        pageSize={24}
        hasMore={false}
        rows={{ from: 49, to: 52 }}
        count={null}
        {...go}
      />
    );
    expect(screen.queryByRole('button', { name: 'next →' })).toBeNull();
    expect(screen.getByText('page 3')).toBeInTheDocument();
  });

  // Item 1b: the list shrank under the reader: an empty page past the first is where it stands
  // and a way back, no rows and no count.
  it('draws an empty page past the end as where it stands and a way back', async () => {
    const user = userEvent.setup();
    const onPrevious = vi.fn();
    render(
      <Pager
        mode="pages"
        label="holders"
        page={2}
        pageSize={20}
        hasMore={false}
        rows={null}
        count={null}
        onPrevious={onPrevious}
        onNext={() => {}}
      />
    );
    expect(text(screen.getByRole('navigation', { name: 'holders' }))).toBe('← prevpage 2next →');
    expect(screen.queryByRole('button', { name: 'next →' })).toBeNull();
    await user.click(screen.getByRole('button', { name: '← prev' }));
    expect(onPrevious).toHaveBeenCalledTimes(1);
  });

  it('refuses figures that do not hold together', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    const at = (over: Record<string, unknown>) => () =>
      render(<Pager mode="pages" label="tokens" {...page3} {...over} {...go} />);
    expect(at({ page: 38 })).toThrow('Pager "tokens": page 38 of 37');
    expect(at({ rows: { from: 61, to: 60 } })).toThrow('Pager "tokens": rows 61–60');
    expect(at({ rows: { from: 41, to: 728 } })).toThrow('Pager "tokens": rows 41–728 of 727');
    expect(at({ rows: { from: 41, to: 61 } })).toThrow('Pager "tokens": 21 rows on a page of 20');
    expect(at({ page: 2.5 })).toThrow('Pager "tokens": page 2.5 is not a whole number');
    expect(at({ count: { total: 0, pages: 0 } })).toThrow('Pager "tokens": 0 pages');
    // an empty first page is no page to draw: a list with no rows draws no pager
    expect(at({ page: 1, rows: null, hasMore: false })).toThrow(
      'Pager "tokens": an empty first page'
    );
  });
});

describe('Pager: a cursor in time', () => {
  it('says its order and goes newer or older; an end it cannot take is quiet', async () => {
    const user = userEvent.setup();
    const onOlder = vi.fn();
    render(
      <Pager
        mode="time"
        label="blocks"
        order="newest first"
        page={1}
        pages={2651295}
        onOlder={onOlder}
      />
    );
    const nav = screen.getByRole('navigation', { name: 'blocks' });
    expect(text(nav)).toBe('newest first← newerpage 1 of 2,651,295older →');
    expect(inkOf(screen.getByText('newest first'))).toBe(QUIET);
    expect(screen.queryByRole('button', { name: '← newer' })).toBeNull();
    expect(inkOf(screen.getByText('← newer'))).toBe(QUIET);
    await user.click(screen.getByRole('button', { name: 'older →' }));
    expect(onOlder).toHaveBeenCalledTimes(1);
  });

  it('writes the page alone where the caller knows no count of pages', () => {
    render(
      <Pager
        mode="time"
        label="feed"
        order="newest first"
        page={4}
        onNewer={() => {}}
        onOlder={() => {}}
      />
    );
    expect(screen.getByText('page 4')).toBeInTheDocument();
    expect(screen.getAllByRole('button').map((b) => b.textContent)).toEqual(['← newer', 'older →']);
  });

  it('draws nothing when neither end can be taken', () => {
    const { container } = render(
      <Pager mode="time" label="feed" order="newest first" page={1} pages={1} />
    );
    expect(container).toBeEmptyDOMElement();
  });
});

describe('Pager: the short list’s own forms', () => {
  it('shows the rest of a short list, and the next of a long one, as ways to go', async () => {
    const user = userEvent.setup();
    const all = vi.fn();
    const next = vi.fn();
    render(
      <Pager
        mode="more"
        all={{ count: 8, singular: 'branch switch', plural: 'branch switches', onShow: all }}
        next={{ count: 50, onShow: next }}
      />
    );
    const buttons = screen.getAllByRole('button');
    expect(buttons.map(text)).toEqual(['all 8 branch switches ↓', 'next 50 ↓']);
    for (const b of buttons) expect(b).toHaveClass('text-jade');
    await user.click(buttons[0]);
    await user.click(buttons[1]);
    expect(all).toHaveBeenCalledTimes(1);
    expect(next).toHaveBeenCalledTimes(1);
  });

  it('groups its counts and agrees its noun', () => {
    render(<Pager mode="more" all={{ count: 1234, singular: 'holder', onShow: () => {} }} />);
    expect(text(screen.getByRole('button'))).toBe('all 1,234 holders ↓');
  });

  it('draws nothing with nothing more to show', () => {
    const { container } = render(<Pager mode="more" />);
    expect(container).toBeEmptyDOMElement();
  });
});
