// Visual finish V5 (resting chroma, 2026-11-04): quiet at rest, light on interaction. Inside a
// table, a feed or a list row a link rests as ink with a faint aqua underline
// (`text-text-bright underline decoration-aqua/35 underline-offset-[3px]`) and carries no hover
// colour of its own: the global `a:hover` (Move 1c, midnight-continued.test) turns it jade with
// the glow. Aqua at rest is kept for the one call to action of a section (`text-interactive`,
// "Charts →"), which sits outside every row. jsdom loads no Tailwind, so these pin the classes;
// the gate script (`aquaAtRest`, computed colour) measures the built pages.
import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { feedSamples } from '@/__tests__/msw/fixtures/home';
import { Address } from '@/components/ui/address';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { ActivityRow } from '@/components/activity/activity-row';
import { ActivityEventGroup, ParticipantLine } from '@/components/activity-event-row';
import { ParticipantRefView } from '@/components/participant-ref';
import { DotCellRecordsTable } from '@/components/identity/dotcell-records-table';
import { ObjectGalleryPanel } from '@/components/object/object-gallery-panel';
import { ActivityStats } from '@/components/home/activity-stats';
import type { Activity, ItemDelta, ParticipantInfo } from '@/lib/api';

const ADDRESS =
  'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq22xdj8q4jdql98qkdhnqzgrsk4nqyavdcratvu9';
const LOCK_HASH = `0x${'5e'.repeat(32)}`;
const TX = `0x${'ab'.repeat(32)}`;

/** The resting style of a link in a row (ruling V5). */
const RESTING = [
  'text-text-bright',
  'underline',
  'decoration-aqua/35',
  'underline-offset-[3px]',
  'transition-colors',
];
/** A class that rests a link in the CTA aqua (#68ccf0: aqua = interactive = info). */
const AQUA_AT_REST = /^(text-aqua|text-interactive|text-info)(\/\d+)?$/;
/** A hover colour that would beat the global jade `a:hover` with aqua. */
const AQUA_HOVER = /^hover:text-(aqua|interactive|info)/;

const classesOf = (el: Element) => Array.from(el.classList);

/** A row link at rest: ink, the faint aqua underline, and no hover colour of its own. */
function expectResting(link: HTMLElement) {
  expect(link.tagName).toBe('A');
  expect(link).toHaveClass(...RESTING);
  expect(classesOf(link).filter((c) => AQUA_AT_REST.test(c))).toEqual([]);
  expect(classesOf(link).filter((c) => c.startsWith('hover:text-'))).toEqual([]);
}

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

describe('resting chroma (V5): links in rows rest as ink and light up on hover', () => {
  it('Address rests as ink with the faint aqua underline; the global a:hover is its hover', () => {
    render(<Address address={ADDRESS} />);
    expectResting(screen.getByRole('link'));
  });

  it('keeps the resting ink when a caller sets only the size (the blocks table miner)', () => {
    render(<Address address={ADDRESS} startChars={10} endChars={6} className="text-small" />);
    const link = screen.getByRole('link');
    expectResting(link);
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
        render: (b) => (
          <Address address={b.miner} startChars={10} endChars={6} className="text-small" />
        ),
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
    expectResting(within(row).getByRole('link', { name: 'ckb1qzda0c...ratvu9' }));
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
    expectResting(within(row).getByRole('link', { name: ADDRESS }));
  });

  it('a party: the standalone form is an Address; a bare lock hash hovers jade, not aqua', () => {
    const party = {
      address: null,
      lockHash: LOCK_HASH,
      lockHashPrefix: null,
      roles: [],
    };
    const { container, rerender } = render(
      <ParticipantRefView participant={{ ...party, address: ADDRESS }} />
    );
    expectResting(screen.getByRole('link'));
    for (const compact of [false, true]) {
      rerender(<ParticipantRefView participant={party} compact={compact} />);
      const link = screen.getByRole('link');
      expectNoAquaLink(container);
      expect(link).toHaveClass('text-text-dim', 'hover:text-jade');
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

  const identities: ItemDelta[] = [
    { kind: 'identity', standard: 'dotcell', identityId: `0x${'ab'.repeat(20)}`, delta: 1 },
    { kind: 'identity', standard: 'dotbit', identityId: `0x${'cd'.repeat(20)}`, delta: -1 },
  ];

  it('an ActivityEventGroup: identity hash links rest as ink; the tx hash hovers jade', () => {
    const activity: Activity = {
      txHash: TX,
      blockNumber: 10_000,
      txIndex: 0,
      timestamp: '1700000000',
      ckbDelta: '-100000000',
      usedDelta: '0',
      isCellbase: false,
      itemDeltas: identities,
      typeCalls: [],
      lockCalls: [],
      protocolActions: [],
      participants: [],
      roles: [],
      tags: 0,
    };
    const { container } = render(
      <div data-feed-row={TX}>
        <ActivityEventGroup activity={activity} formatTimeAgo={() => '2 hrs ago'} />
      </div>
    );
    expectNoAquaLink(container);
    const hashLinks = screen
      .getAllByRole('link')
      .filter((a) => a.getAttribute('href')?.includes('/identities/'));
    // The group draws its narrow card and its wide grid cells, each with both identities.
    expect(hashLinks).toHaveLength(2 * identities.length);
    hashLinks.forEach(expectResting);
    const txLinks = screen
      .getAllByRole('link')
      .filter((a) => a.getAttribute('href')?.endsWith(`/tx/${TX}`));
    expect(txLinks).toHaveLength(2);
    for (const tx of txLinks) expect(tx).toHaveClass('text-text', 'hover:text-jade');
  });

  it("a participant line: an identity delta's hash link rests as ink", () => {
    const { container } = render(
      <ParticipantLine
        participant={{
          address: null,
          lockHash: LOCK_HASH,
          lockHashPrefix: null,
          roles: [],
          ckbDelta: '0',
          usedDelta: '0',
          itemDeltas: identities,
          tags: 0,
        }}
      />
    );
    expectNoAquaLink(container);
    const hashLinks = screen
      .getAllByRole('link')
      .filter((a) => a.getAttribute('href')?.includes('/identities/'));
    expect(hashLinks).toHaveLength(2);
    hashLinks.forEach(expectResting);
  });

  it('an identity card in the object gallery: its label link rests as ink', () => {
    render(
      <ObjectGalleryPanel
        totalCount={1}
        isLoading={false}
        page={1}
        hasPrevious={false}
        onNext={() => {}}
        onPrevious={() => {}}
        collectionItems={{
          data: [
            {
              nftId: `0x${'cd'.repeat(20)}`,
              name: 'badger.bit',
              standard: 'dotbit',
              ownerLockHash: LOCK_HASH,
              ownerAddress: ADDRESS,
              isLive: true,
              createdAtBlock: 10_000,
            },
          ],
          limit: 18,
          hasMore: false,
          nextCursor: null,
        }}
      />
    );
    const label = screen.getByRole('link', { name: 'badger.bit' });
    expectResting(label);
    expect(label).toHaveClass('font-semibold');
  });

  it('a section CTA outside every row keeps its aqua (text-interactive) — untouched', () => {
    stubChartWidth(560);
    render(<ActivityStats />);
    const cta = screen.getByRole('link', { name: 'Charts →' });
    expect(cta).toHaveClass('text-interactive', 'hover:text-jade');
    expect(cta).not.toHaveClass('underline', 'decoration-aqua/35');
    expect(cta.closest('table, [role="row"], [data-feed-row]')).toBeNull();
  });
});
