import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import userEvent from '@testing-library/user-event';
import { render, screen, within } from '@testing-library/react';
import { TabLinks, Tabs, type Tab, type TabLink } from '@/components/ui/tabs';
import { tabRowPx } from '@/lib/control-row-fit';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { atViewportWidth } from '@/__tests__/utils/viewport';
import { inkOf, BRIGHT, QUIET, READING } from '@/__tests__/utils/ink-role';
import {
  containerContentPx,
  contentPx,
  flexWrapsAt,
  targetHeightPx,
} from '@/__tests__/utils/tailwind-box';

/**
 * The tab row of the controls board (gate D0′, passed 2026-11-19): `role="tablist"`, one stop in
 * the tab order (the current tab), arrows, Home and End; the names 17 px from sm and 14 px below,
 * the reading ink, the current one bright and 600 on the 2 px celadon line; a count 12 px in the
 * quiet ink; a zero's name quiet. As a tier-1 section's command line it carries the prompt square
 * and lowercase names. The phone rule: the counts leave a row wider than its line.
 */

type Key = 'activities' | 'transactions' | 'cells';
const ADDRESS: Tab<Key>[] = [
  { key: 'activities', label: 'activities', count: 3 },
  { key: 'transactions', label: 'transactions', count: 3 },
  { key: 'cells', label: 'live cells', count: 3 },
];

function Row({ tabs = ADDRESS, command = true }: { tabs?: Tab<Key>[]; command?: boolean }) {
  const [value, setValue] = useState<Key>('activities');
  return (
    <Tabs label="address" tabs={tabs} value={value} onChange={setValue} command={command}>
      <p>the {value} list</p>
    </Tabs>
  );
}

/** The drawn row's width to the hundredth of a pixel the budgets are written in. */
const drawnPx = (list: Element, viewport: number) =>
  Math.round(contentPx(list, viewport) * 100) / 100;

/** A window `viewport` px wide whose tab row stands on the page's full line. */
function at(viewport: number) {
  atViewportWidth(viewport);
  stubChartWidth(containerContentPx(viewport));
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

/**
 * The row's panel, the same in either form (lane cd-fixes-controls, review-fix-logic-pages NOTE 4):
 * a tab panel named by the current view, one stop of the tab order, the panel its control points
 * at. Its narrow form drew a div with no role whose `aria-label` named nothing.
 */
function expectPanel(view: string, text: string, control: HTMLElement) {
  const panel = screen.getByRole('tabpanel', { name: view });
  expect(panel).toHaveTextContent(text);
  expect(panel).toHaveAttribute('tabindex', '0');
  expect(control).toHaveAttribute('aria-controls', panel.id);
}

describe('Tabs: roles, names and the panel', () => {
  it('is a tablist of tabs named by their words, the panel labelled by the current one', () => {
    render(<Row />);
    const list = screen.getByRole('tablist', { name: 'address' });
    const tabs = within(list).getAllByRole('tab');
    expect(tabs.map((t) => t.textContent)).toEqual([
      'activities 3',
      'transactions 3',
      'live cells 3',
    ]);
    expect(screen.getByRole('tab', { name: 'activities 3' })).toHaveAttribute(
      'aria-selected',
      'true'
    );
    const panel = screen.getByRole('tabpanel');
    expect(panel).toHaveTextContent('the activities list');
    expect(tabs[0]).toHaveAttribute('aria-controls', panel.id);
    expect(panel).toHaveAttribute('aria-labelledby', tabs[0].id);
    expectPanel('activities 3', 'the activities list', tabs[0]);
    expect(tabs.slice(1).map((t) => t.getAttribute('aria-selected'))).toEqual(['false', 'false']);
  });

  it('refuses a value that is none of its tabs', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() =>
      render(
        <Tabs label="address" tabs={ADDRESS} value={'names' as Key} onChange={() => {}}>
          <p />
        </Tabs>
      )
    ).toThrow('Tabs "address": no tab names');
  });
});

describe('Tabs: the keyboard', () => {
  it('is one stop in the tab order, the current tab', async () => {
    render(<Row />);
    const tabs = screen.getAllByRole('tab');
    expect(tabs.map((t) => t.tabIndex)).toEqual([0, -1, -1]);
    await userEvent.tab();
    expect(tabs[0]).toHaveFocus();
  });

  it('moves and selects with the arrows, wrapping, and with Home and End', async () => {
    const user = userEvent.setup();
    render(<Row />);
    const tab = (name: RegExp) => screen.getByRole('tab', { name });
    await user.tab();
    await user.keyboard('{ArrowRight}');
    expect(tab(/^transactions/)).toHaveFocus();
    expect(tab(/^transactions/)).toHaveAttribute('aria-selected', 'true');
    expect(screen.getByRole('tabpanel')).toHaveTextContent('the transactions list');
    await user.keyboard('{ArrowRight}{ArrowRight}');
    expect(tab(/^activities/)).toHaveFocus();
    await user.keyboard('{ArrowLeft}');
    expect(tab(/^live cells/)).toHaveAttribute('aria-selected', 'true');
    await user.keyboard('{Home}');
    expect(tab(/^activities/)).toHaveFocus();
    await user.keyboard('{End}');
    expect(tab(/^live cells/)).toHaveFocus();
    // a tab row's line is left to right: the up and down arrows are the page's
    await user.keyboard('{ArrowDown}');
    expect(tab(/^live cells/)).toHaveAttribute('aria-selected', 'true');
  });

  it('selects on a click', async () => {
    const user = userEvent.setup();
    render(<Row />);
    await user.click(screen.getByRole('tab', { name: /^live cells/ }));
    expect(screen.getByRole('tabpanel')).toHaveTextContent('the cells list');
  });
});

describe('Tabs: the board’s look', () => {
  it('draws the current tab bright and 600 on the celadon line, the others in the reading ink', () => {
    render(<Row />);
    const [current, other] = screen.getAllByRole('tab');
    expect(current).toHaveClass('border-b-2', 'border-jade', 'font-semibold');
    expect(inkOf(current)).toBe(BRIGHT);
    expect(other).toHaveClass('border-b-2', 'border-transparent', 'font-normal');
    expect(inkOf(other)).toBe(READING);
    // the count: 12 px in the quiet ink, never bright, never 600
    const count = within(current).getByText('3');
    expect(count).toHaveClass('text-meta', 'font-normal');
    expect(inkOf(count)).toBe(QUIET);
  });

  it('draws a zero’s name in the quiet ink, never faded', () => {
    render(<Row tabs={[ADDRESS[0], { ...ADDRESS[1], count: 0 }, ADDRESS[2]]} />);
    const zero = screen.getByRole('tab', { name: 'transactions 0' });
    expect(inkOf(zero)).toBe(QUIET);
    expect(zero.className).not.toMatch(/opacity/);
  });

  it('is a command line with the prompt square and lowercase names, and plain without', () => {
    const { container, unmount } = render(<Row />);
    const square = container.querySelector('[role="tablist"] > .section-mark');
    expect(square).toHaveAttribute('aria-hidden', 'true');
    expect(screen.getAllByRole('tab').every((t) => t.classList.contains('lowercase'))).toBe(true);
    unmount();
    const plain = render(<Row command={false} />);
    expect(plain.container.querySelector('.section-mark')).toBeNull();
    expect(screen.getAllByRole('tab').some((t) => t.classList.contains('lowercase'))).toBe(false);
  });

  it('is 17 px from sm and 14 px below it, on one line that never wraps or scrolls', () => {
    render(<Row />);
    const list = screen.getByRole('tablist');
    for (const tab of screen.getAllByRole('tab'))
      expect(tab).toHaveClass('text-body', 'sm:text-section');
    expect(flexWrapsAt(list, 360)).toBe(false);
    expect(list.className).not.toMatch(/overflow|truncate/);
  });

  it('gives a tab a 32 px target at every width', () => {
    render(<Row />);
    for (const w of [1280, 640, 390, 360])
      for (const tab of screen.getAllByRole('tab')) expect(targetHeightPx(tab, w)).toBe(32);
  });
});

describe('Tabs: the phone rule, by the one function (lib/control-row-fit)', () => {
  it('draws the row the function measures: 428 px at 1280, 356.4 px at 390 with the counts', () => {
    at(1280);
    const wide = render(<Row />);
    expect(drawnPx(screen.getByRole('tablist'), 1280)).toBe(
      tabRowPx(ADDRESS, 'section', true).withCounts
    );
    expect(tabRowPx(ADDRESS, 'section', true).withCounts).toBe(428);
    wide.unmount();
    at(390);
    render(<Row />);
    expect(screen.getAllByRole('tab').map((t) => t.textContent)).toEqual([
      'activities 3',
      'transactions 3',
      'live cells 3',
    ]);
    expect(drawnPx(screen.getByRole('tablist'), 390)).toBe(356.4);
  });

  it('lets the counts leave the address row at 360 (356.4 px in 328), its names whole', () => {
    at(360);
    render(<Row />);
    const list = screen.getByRole('tablist');
    expect(screen.getAllByRole('tab').map((t) => t.textContent)).toEqual([
      'activities',
      'transactions',
      'live cells',
    ]);
    expect(drawnPx(list, 360)).toBe(tabRowPx(ADDRESS, 'body', true).bare);
    expect(drawnPx(list, 360)).toBeLessThanOrEqual(328);
  });

  it('keeps the inventory’s counts at 360 (324 px in 328)', () => {
    at(360);
    render(
      <Tabs
        label="inventory"
        command
        tabs={[
          { key: 'tokens', label: 'tokens', count: 727 },
          { key: 'objects', label: 'objects', count: 5653 },
          { key: 'identities', label: 'identities', count: 4 },
        ]}
        value="tokens"
        onChange={() => {}}
      >
        <p />
      </Tabs>
    );
    expect(screen.getAllByRole('tab').map((t) => t.textContent)).toEqual([
      'tokens 727',
      'objects 5,653',
      'identities 4',
    ]);
    expect(drawnPx(screen.getByRole('tablist'), 360)).toBe(324);
  });

  // re-pinned on purpose (the lead's final round, item 4): a row whose names alone are wider than
  // the narrowest line no longer throws; it is tabs where its line holds it, and its narrow form,
  // the library's Select, where it does not (it threw at every width)
  it('keeps a row wider than the narrowest line as tabs where its line holds it', () => {
    at(1280);
    render(<Row tabs={[...ADDRESS, { key: 'names' as Key, label: 'names', count: 1 }]} />);
    expect(screen.getAllByRole('tab')).toHaveLength(4);
  });
});

/**
 * The lead's final round, item 4: at 320 px the transactions page's row ("transactions · cellbases ·
 * in the pool") and the address page's ran 13 px past the window. A tab row decides by measurement
 * from its LABELS whether it fits the line it is given (the counts leave first, as before); on a
 * line that cannot hold its names it takes its narrow form, the library's Select after "showing",
 * never a clipped, wrapped or scrolled row. The arithmetic, a command row at the body step below
 * 640 (14 px, a glyph 8.4 px; the square 8 less its 8 px pull, then a 16 px gap; 16 px between
 * tabs): each row's names are 32 glyphs, 268.8 px, with 48 px of chrome, 316.8 px. A 320 px
 * window's line is 288 px (316.8 is 28.8 over it, 12.8 past the window): the Select. A 360 px
 * window's is 328: both stay tabs, the address row without its counts (356.4 px with them).
 */
describe('Tabs: the narrow form, where the line cannot hold the names', () => {
  type View = 'committed' | 'cellbases' | 'pool';
  const VIEWS: Tab<View>[] = [
    { key: 'committed', label: 'transactions' },
    { key: 'cellbases', label: 'cellbases' },
    { key: 'pool', label: 'in the pool' },
  ];
  function Views() {
    const [value, setValue] = useState<View>('committed');
    return (
      <Tabs label="Transaction views" command tabs={VIEWS} value={value} onChange={setValue}>
        <p>the {value} list</p>
      </Tabs>
    );
  }

  it('measures both rows from their labels: 316.8 px, over a 320 window’s 288 and within a 360’s 328', () => {
    expect(tabRowPx(VIEWS, 'body', true).bare).toBe(316.8);
    expect(tabRowPx(ADDRESS, 'body', true).bare).toBe(316.8);
    expect(containerContentPx(320)).toBe(288);
    expect(containerContentPx(360)).toBe(328);
  });

  // Re-pinned on purpose (lane cd-fixes-controls, review-fix-logic-pages NOTE 4): the Select is
  // named by the row's label, as its tablist is, the word it stands after still "showing" (it was
  // named "showing", so two such rows on one page had one name); its panel is the tab form's
  it('takes the Select after "showing" at 320, its options the tabs, and chooses through it', async () => {
    const user = userEvent.setup();
    at(320);
    render(<Views />);
    expect(screen.queryByRole('tablist')).toBeNull();
    const select = screen.getByRole<HTMLSelectElement>('combobox', { name: 'Transaction views' });
    expect([...(select.labels ?? [])].map((l) => l.textContent)).toEqual(['showing']);
    expect(select).toHaveValue('committed');
    expect(
      within(select)
        .getAllByRole('option')
        .map((o) => o.textContent)
    ).toEqual(['transactions', 'cellbases', 'in the pool']);
    expectPanel('transactions', 'the committed list', select);
    await user.selectOptions(select, 'pool');
    expectPanel('in the pool', 'the pool list', select);
  });

  it('takes the Select for the address row at 320 too, counts and all', () => {
    at(320);
    render(<Row />);
    expect(screen.queryByRole('tablist')).toBeNull();
    expect(screen.getByRole('combobox', { name: 'address' })).toHaveValue('activities');
  });

  it('tells two narrow rows on one page apart by their labels', () => {
    at(320);
    render(
      <>
        <Views />
        <Row />
      </>
    );
    expect(screen.getAllByRole('combobox').map((c) => c.getAttribute('aria-label'))).toEqual([
      'Transaction views',
      'address',
    ]);
    expect(screen.getAllByRole('tabpanel').map((p) => p.getAttribute('aria-label'))).toEqual([
      'transactions',
      'activities',
    ]);
  });

  it('stays tabs at 360: the transactions row whole, the address row without its counts', () => {
    at(360);
    const views = render(<Views />);
    expect(screen.getAllByRole('tab').map((t) => t.textContent)).toEqual([
      'transactions',
      'cellbases',
      'in the pool',
    ]);
    expect(screen.queryByRole('combobox')).toBeNull();
    views.unmount();
    render(<Row />);
    expect(screen.getAllByRole('tab').map((t) => t.textContent)).toEqual([
      'activities',
      'transactions',
      'live cells',
    ]);
  });
});

/**
 * Lane c2-finish, item 3: a tab that is its own address is a link. The inventory's classes are three
 * routes, so as tabs a class could not open in a new tab and an arrow key changed the route,
 * remounting the page and losing the focus. `TabLinks` is the tab row's form for that case: the
 * same look, a `nav` of links (WAI-ARIA navigation, not a tablist), the current one
 * `aria-current="page"`, each link a stop of the tab order and no arrow keys of its own.
 */
describe('TabLinks: a tab that is its own address', () => {
  type Class = 'tokens' | 'objects' | 'identities';
  const CLASSES: TabLink<Class>[] = [
    { key: 'tokens', label: 'tokens', count: 727, href: '/inventory/tokens' },
    { key: 'objects', label: 'objects', count: 5653, href: '/inventory/objects' },
    { key: 'identities', label: 'identities', count: 0, href: '/inventory/identities' },
  ];
  const Classes = ({ command = true }: { command?: boolean }) => (
    <TabLinks label="Inventory classes" tabs={CLASSES} current="tokens" command={command} />
  );

  it('is a navigation of links named by their words, the current one the page', () => {
    render(<Classes />);
    const nav = screen.getByRole('navigation', { name: 'Inventory classes' });
    expect(screen.queryByRole('tablist')).toBeNull();
    expect(screen.queryByRole('tab')).toBeNull();
    const links = within(nav).getAllByRole('link');
    expect(links.map((l) => l.textContent)).toEqual([
      'tokens 727',
      'objects 5,653',
      'identities 0',
    ]);
    expect(links.map((l) => l.getAttribute('href'))).toEqual([
      expect.stringMatching(/\/inventory\/tokens$/),
      expect.stringMatching(/\/inventory\/objects$/),
      expect.stringMatching(/\/inventory\/identities$/),
    ]);
    expect(links.map((l) => l.getAttribute('aria-current'))).toEqual(['page', null, null]);
  });

  it('is a stop of the tab order per link, and leaves the arrows to the page', async () => {
    const user = userEvent.setup();
    render(<Classes />);
    const links = screen.getAllByRole('link');
    await user.tab();
    expect(links[0]).toHaveFocus();
    await user.keyboard('{ArrowRight}');
    expect(links[0]).toHaveFocus();
    await user.tab();
    expect(links[1]).toHaveFocus();
  });

  it('draws the tab row’s look: the current bright and 600 on the celadon line, a zero quiet', () => {
    render(<Classes />);
    const [current, other, zero] = screen.getAllByRole('link');
    const name = (link: HTMLElement) => link.firstElementChild!;
    expect(current).toHaveClass('border-b-2', 'border-jade', 'font-semibold');
    expect(inkOf(name(current))).toBe(BRIGHT);
    expect(other).toHaveClass('border-b-2', 'border-transparent', 'font-normal');
    expect(inkOf(name(other))).toBe(READING);
    expect(inkOf(name(zero))).toBe(QUIET);
    const count = within(current).getByText('727');
    expect(count).toHaveClass('text-meta', 'font-normal');
    expect(inkOf(count)).toBe(QUIET);
    // the tab's box: the same classes as a tab's of the same role. Re-pinned (lane c2-finish,
    // rulings round 2): a row's size goes by its role, so the command links are set beside a
    // command tab row (it compared them with a plain one, the same size then)
    render(
      <Tabs label="x" tabs={CLASSES} value="tokens" onChange={() => {}} command>
        <p />
      </Tabs>
    );
    const tab = screen.getAllByRole('tab')[0];
    for (const cls of ['text-body', 'sm:text-section', 'h-8', 'border-b-2', 'pb-1.5', '-mb-px'])
      expect(current.classList.contains(cls), cls).toBe(tab.classList.contains(cls));
  });

  it('gives a link a 32 px target, on one line with the prompt square and lowercase names', () => {
    const { container } = render(<Classes />);
    for (const w of [1280, 640, 390, 360])
      for (const link of screen.getAllByRole('link')) expect(targetHeightPx(link, w)).toBe(32);
    const nav = screen.getByRole('navigation');
    expect(nav.querySelector(':scope > .section-mark')).toHaveAttribute('aria-hidden', 'true');
    expect(screen.getAllByRole('link').every((l) => l.classList.contains('lowercase'))).toBe(true);
    expect(flexWrapsAt(nav, 360)).toBe(false);
    expect(container.querySelector('[role="tabpanel"]')).toBeNull();
  });

  it('keeps the phone rule of the one function: 324 px at 360 with the counts, which stay', () => {
    at(360);
    render(<Classes />);
    expect(drawnPx(screen.getByRole('navigation'), 360)).toBe(
      tabRowPx(CLASSES, 'body', true).withCounts
    );
    expect(screen.getAllByRole('link').map((l) => l.textContent)).toEqual([
      'tokens 727',
      'objects 5,653',
      'identities 0',
    ]);
  });

  it('refuses a current that is none of its links', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() =>
      render(<TabLinks label="Inventory classes" tabs={CLASSES} current={'names' as Class} />)
    ).toThrow('TabLinks "Inventory classes": no link names');
  });
});

/**
 * The owner's ruling (2026-11-19, rulings round 2 of lane c2-finish): a tab row's size goes by its
 * role. A command row IS its section's heading, so it takes the heading's size (17 px from 640, 14
 * below); a plain row stands inside a section under that section's own heading, so it takes the
 * body step (14 px) at every width: a 17 px row under a 17 px heading read as two headings, one
 * over the other (the home feed under "feed --last 8"). The count, the 32 px target, the selected
 * state and the phone rule are the same; a plain row's fit is the body step's at every width.
 */
describe('Tabs and TabLinks: a plain row takes the body step at every width', () => {
  it('sets a plain row’s names at the body step and never the section step, with its gap', () => {
    render(<Row command={false} />);
    const list = screen.getByRole('tablist');
    for (const tab of screen.getAllByRole('tab')) {
      expect(tab).toHaveClass('text-body', 'pb-1.5');
      expect(tab.className).not.toMatch(/sm:text-section|sm:pb-/);
      for (const w of [1280, 640, 390, 360]) expect(targetHeightPx(tab, w)).toBe(32);
    }
    expect(list.className).not.toMatch(/sm:gap-/);
    // the body step's row at 1280: 32 glyphs at 8.4 px, three counts, two gaps of 16 px
    expect(drawnPx(list, 1280)).toBe(tabRowPx(ADDRESS, 'body', false).withCounts);
    expect(tabRowPx(ADDRESS, 'body', false).withCounts).toBe(340.4);
  });

  it('keeps a command row at the section step from 640 and the body step below it', () => {
    render(<Row />);
    for (const tab of screen.getAllByRole('tab'))
      expect(tab).toHaveClass('text-body', 'sm:text-section', 'pb-1.5', 'sm:pb-[7px]');
    expect(screen.getByRole('tablist')).toHaveClass('gap-4', 'sm:gap-[22px]');
  });

  it('fits a plain row at the body step from 640 too: its counts stay where the body row fits', () => {
    // a 380 px line at 1280: the body row with its counts is 340.4 px, the section row 410
    atViewportWidth(1280);
    stubChartWidth(380);
    const plain = render(<Row command={false} />);
    expect(screen.getAllByRole('tab').map((t) => t.textContent)).toEqual([
      'activities 3',
      'transactions 3',
      'live cells 3',
    ]);
    plain.unmount();
    render(
      <Tabs label="address" tabs={ADDRESS} value="activities" onChange={() => {}} command>
        <p />
      </Tabs>
    );
    // a command row there sets its names at 17 px: 428 px with its counts, past 380, and 388.4
    // without them (32 glyphs of 10.2 px, 326.4, and 62 px of chrome), past 380 too: re-pinned on
    // purpose (the lead's final round, item 4), its narrow form, where it drew its names 8.4 px
    // past its line
    expect(tabRowPx(ADDRESS, 'section', true).bare).toBe(388.4);
    expect(screen.queryByRole('tablist')).toBeNull();
    // named by its row's label (re-pinned on purpose, review-fix-logic-pages NOTE 4)
    expect(screen.getByRole('combobox', { name: 'address' })).toHaveValue('activities');
  });

  it('draws a plain TabLinks row at the body step too', () => {
    render(
      <TabLinks
        label="classes"
        tabs={[
          { key: 'tokens', label: 'tokens', count: 727, href: '/inventory/tokens' },
          { key: 'objects', label: 'objects', count: 5653, href: '/inventory/objects' },
        ]}
        current="tokens"
      />
    );
    for (const link of screen.getAllByRole('link')) {
      expect(link).toHaveClass('text-body');
      expect(link.className).not.toMatch(/sm:text-section/);
    }
  });
});
