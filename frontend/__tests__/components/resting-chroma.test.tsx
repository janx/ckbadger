import { linkLookClass } from '@/components/ui/link';
// Visual finish V5 (resting chroma, 2026-11-04): quiet at rest, light on interaction. Inside a
// table, a feed or a list row a link rests as ink with a faint aqua underline
// (`text-text-bright underline decoration-aqua/35 underline-offset-[3px]`) and carries no hover
// colour of its own: the global `a:hover` (Move 1c, midnight-continued.test) turns it jade with
// the glow. Aqua at rest is kept for the one call to action of a section (`text-interactive`,
// "charts →"), which sits outside every row. jsdom loads no Tailwind, so these pin the classes;
// the gate script (`aquaAtRest`, computed colour) measures the built pages.
import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { AQUA_AT_REST } from '@/__tests__/utils/resting-link';
import type { LinkLook } from '@/components/ui/link';
import { feedSamples } from '@/__tests__/msw/fixtures/home';
import { Address } from '@/components/ui/address';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { ActivityRow } from '@/components/activity/activity-row';
import { ParticipantRefView } from '@/components/participant-ref';
import { DotCellRecordsTable } from '@/components/identity/dotcell-records-table';
import { ActivityStats } from '@/components/home/activity-stats';
import type { ParticipantInfo } from '@/lib/api';

const ADDRESS =
  'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq22xdj8q4jdql98qkdhnqzgrsk4nqyavdcratvu9';
const LOCK_HASH = `0x${'5e'.repeat(32)}`;
const TX = `0x${'ab'.repeat(32)}`;

/** A hover colour that would beat the global jade `a:hover` with aqua. */
const AQUA_HOVER = /^hover:text-(aqua|interactive|info)/;

const classesOf = (el: Element) => Array.from(el.classList);

/** Every link inside `scope`: never aqua at rest, never an aqua hover (one hover colour, jade). */
function expectNoAquaLink(scope: Element) {
  const links = Array.from(scope.querySelectorAll('a'));
  expect(links.length).toBeGreaterThan(0);
  for (const a of links)
    expect(
      classesOf(a).filter((c) => AQUA_AT_REST.test(c) || AQUA_HOVER.test(c)),
      `${a.textContent} → ${a.getAttribute('href')}`
    ).toEqual([]);
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

/**
 * A link that states its look rests in that look's ink (components/ui/link.tsx), with no aqua at
 * rest and no hover colour of its own: the global `a:hover` lights it.
 */
function expectLookedLink(link: HTMLElement, look: LinkLook) {
  expect(link.tagName).toBe('A');
  expect(link).toHaveClass(...linkLookClass(look)!.split(' '));
  const classes = Array.from(link.classList);
  expect(classes.filter((c) => AQUA_AT_REST.test(c))).toEqual([]);
  expect(classes.filter((c) => c.startsWith('hover:text-'))).toEqual([]);
}

describe('resting chroma (V5): links in rows rest as ink and light up on hover', () => {
  // Re-pinned on purpose (lane cd-fixes-controls, review-p-tests SHOULD-4): an Address states its
  // look, which is required; the V5 resting look (bright over a faint aqua underline) was what an
  // Address drew for a caller that stated none, and went with the last of them. It rests in its
  // look's ink, with no aqua and no hover colour of its own (it was "Address rests as ink with the
  // faint aqua underline; the global a:hover is its hover")
  it('Address rests in the look its caller states; the global a:hover is its hover', () => {
    render(<Address look="inline" address={ADDRESS} size="row" />);
    expectLookedLink(screen.getByRole('link'), 'inline');
  });

  // re-pinned on purpose, as above: the blocks table's miner is a row's key at the small step
  it('keeps its look when a caller sets only the size (the blocks table miner)', () => {
    render(<Address look="key" address={ADDRESS} size="row" className="text-small" />);
    const link = screen.getByRole('link');
    expectLookedLink(link, 'key');
    expect(link).toHaveClass('text-small');
    expect(link).not.toHaveClass('text-body');
  });

  it('a ListTable row with an Address cell: the row key leads, the miner link rests as ink', () => {
    stubChartWidth(1200);
    type Block = { number: number; miner: string };
    const columns: ListColumn<Block>[] = [
      {
        key: 'height',
        label: 'Height',
        minWidth: 112,
        render: (b) => <span className="text-text-bright text-body">#{b.number}</span>,
      },
      {
        key: 'miner',
        label: 'Miner',
        minWidth: 200,
        render: (b) => <Address look="key" address={b.miner} size="row" className="text-small" />,
      },
    ];
    render(
      <ListTable
        label="Blocks"
        columns={columns}
        rows={[{ number: 20919087, miner: ADDRESS }]}
        rowKey={(b) => String(b.number)}
        mobileRow={(b) => <span>{b.number}</span>}
      />
    );
    const table = screen.getByRole('table', { name: 'Blocks' });
    const [, row] = within(table).getAllByRole('row');
    expectNoAquaLink(row);
    // R9 (re-pinned): the row size is `ckb1…` and the last 8 characters
    // re-pinned on purpose (lane cd-fixes-controls, SHOULD-4): its look's ink, as above
    expectLookedLink(within(row).getByRole('link', { name: 'ckb1…dcratvu9' }), 'key');
  });

  it('a .cell records table row: its decoded address link rests as ink', () => {
    render(
      <DotCellRecordsTable
        records={[
          {
            key: 'address.309',
            label: '',
            valueHex: '0x636b7431',
            valueUtf8: ADDRESS,
            ttl: 300,
            decodedAddress: { address: ADDRESS, lockHash: LOCK_HASH },
          },
        ]}
      />
    );
    const [, row] = screen.getAllByRole('row');
    expectNoAquaLink(row);
    // re-pinned on purpose (lane p-assets, the link-look gate): a link in the row states its look,
    // inline, the line's ink over the muted underline of components/ui/link.tsx (it drew V5's
    // resting look by hand: bright ink over a faint aqua underline); no hover colour of its own
    const link = within(row).getByRole('link', { name: ADDRESS });
    expect(link).toHaveClass(...linkLookClass('inline')!.split(' '));
    expect(Array.from(link.classList).filter((c) => c.startsWith('hover:text-'))).toEqual([]);
  });

  it('a party: the standalone form is an Address; a bare lock hash hovers jade, not aqua', () => {
    const party = {
      address: null,
      lockHash: LOCK_HASH,
      lockHashPrefix: null,
      roles: [],
    };
    // re-pinned on purpose (lane cd-fixes-controls, SHOULD-4): the standalone form states its
    // Address's look, which is required (it drew the resting look of a caller that stated none)
    const { container, rerender } = render(
      <ParticipantRefView participant={{ ...party, address: ADDRESS }} size="row" look="inline" />
    );
    expectLookedLink(screen.getByRole('link'), 'inline');
    for (const compact of [false, true]) {
      rerender(<ParticipantRefView participant={party} size="row" compact={compact} />);
      const link = screen.getByRole('link');
      expectNoAquaLink(container);
      // re-pinned with readability ruling R3: a link reads in the reading ink (it was dim)
      expect(link).toHaveClass('text-text', 'hover:text-jade');
    }
  });

  it('an ActivityRow (the feed: already ink at rest) has no aqua link, at rest or on hover', () => {
    const lockOnly: ParticipantInfo = {
      address: null,
      lockHash: LOCK_HASH,
      lockHashPrefix: null,
      lockKind: null,
      roles: [],
      ckbDelta: '-100000000',
      usedDelta: '0',
      itemDeltas: [],
      tags: 0,
    };
    for (const sample of Object.values(feedSamples)) {
      const { container, unmount } = render(
        <ActivityRow activity={{ ...sample, participants: [...sample.participants, lockOnly] }} />
      );
      const row = container.querySelector('[data-feed-row]');
      expect(row).not.toBeNull();
      expectNoAquaLink(row!);
      unmount();
    }
  });
  // Re-pinned on purpose (lists lane P3, the link-look gate: a link's ink is its look's): the
  // section's call to action is a way onward, celadon, its arrow the look's; it kept the aqua
  // text-interactive by hand (renamed: "… keeps its aqua (text-interactive) — untouched")
  it('a section CTA outside every row is a way onward, celadon — no row link', () => {
    stubChartWidth(560);
    render(<ActivityStats />);
    const cta = screen.getByRole('link', { name: /^charts/ });
    expect(cta).toHaveClass('text-jade');
    expect(cta).not.toHaveClass('text-interactive', 'hover:text-jade');
    expect(cta).not.toHaveClass('underline', 'decoration-aqua/35');
    expect(cta.closest('table, [role="row"], [data-feed-row]')).toBeNull();
  });
});
