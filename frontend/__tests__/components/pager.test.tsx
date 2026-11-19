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

describe('Pager: pages', () => {
  const page3 = { from: 41, to: 60, total: 727, pageSize: 20, page: 3, pages: 37 };

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
    render(
      <Pager mode="pages" label="tokens" {...page3} onPrevious={() => {}} onNext={() => {}} />
    );
    expect(inkOf(screen.getByText(/^41–60 of 727/))).toBe(QUIET);
    expect(inkOf(screen.getByText('page 3 of 37'))).toBe(READING);
    for (const go of screen.getAllByRole('button')) {
      expect(go).toHaveClass('text-jade', 'text-small');
      expect(go.className).not.toMatch(/\bborder\b|\bbg-/);
    }
  });

  it('draws an end that cannot be taken in the quiet ink, and not as a control', () => {
    const { unmount } = render(
      <Pager
        mode="pages"
        label="tokens"
        {...page3}
        page={1}
        from={1}
        to={20}
        onPrevious={() => {}}
        onNext={() => {}}
      />
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
        from={721}
        to={727}
        onPrevious={() => {}}
        onNext={() => {}}
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
        from={1}
        to={8}
        total={8}
        pageSize={20}
        page={1}
        pages={1}
        onPrevious={() => {}}
        onNext={() => {}}
      />
    );
    expect(container).toBeEmptyDOMElement();
  });

  it('groups every figure and computes none: it writes the pages it is given', () => {
    render(
      <Pager
        mode="pages"
        label="blocks"
        from={2000001}
        to={2000020}
        total={53025887}
        pageSize={20}
        page={100001}
        // the caller's figure, not ⌈53,025,887 / 20⌉ = 2,651,295: the pager divides nothing
        pages={2651300}
        onPrevious={() => {}}
        onNext={() => {}}
      />
    );
    expect(screen.getByText('page 100,001 of 2,651,300')).toBeInTheDocument();
    expect(text(screen.getByText(/^2,000,001–2,000,020 of 53,025,887/))).toBe(
      '2,000,001–2,000,020 of 53,025,887 · 20 a page'
    );
  });

  it('keeps “· 20 a page” from 640 and leaves it below, the phone’s line', () => {
    render(
      <Pager mode="pages" label="tokens" {...page3} onPrevious={() => {}} onNext={() => {}} />
    );
    const perPage = screen.getByText(/· 20 a page/);
    expect(inFlowAt(perPage, 640)).toBe(true);
    expect(inFlowAt(perPage, 390)).toBe(false);
  });

  it('gives a way to go a 28 px target from 640 and 32 below', () => {
    render(
      <Pager mode="pages" label="tokens" {...page3} onPrevious={() => {}} onNext={() => {}} />
    );
    for (const go of screen.getAllByRole('button'))
      expect([1280, 640, 390, 360].map((w) => targetHeightPx(go, w))).toEqual([28, 28, 32, 32]);
  });

  it('refuses figures that do not hold together', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    const go = { onPrevious: () => {}, onNext: () => {} };
    const at = (over: Partial<typeof page3>) => () =>
      render(<Pager mode="pages" label="tokens" {...page3} {...over} {...go} />);
    expect(at({ page: 38 })).toThrow('Pager "tokens": page 38 of 37');
    expect(at({ from: 61 })).toThrow('Pager "tokens": rows 61–60 of 727');
    expect(at({ to: 728 })).toThrow('Pager "tokens": rows 41–728 of 727');
    expect(at({ to: 61 })).toThrow('Pager "tokens": 21 rows on a page of 20');
    expect(at({ page: 2.5 })).toThrow('Pager "tokens": page 2.5 is not a whole number');
    expect(at({ pages: 0 })).toThrow('Pager "tokens": 0 pages');
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
