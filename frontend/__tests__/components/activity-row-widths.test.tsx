// Responsive fix plan 2026-11-01, lane A: the activity participant grid across widths. jsdom does
// not lay out, so these assert the structure and the breakpoint classes that decide the layout.
import { render, within } from '@/__tests__/utils/test-utils';
import { ActivityRow, type ActivityRowData } from '@/components/activity/activity-row';
import type { ItemDelta, ParticipantInfo } from '@/lib/api';

function party(over: Partial<ParticipantInfo> = {}): ParticipantInfo {
  return {
    lockHash: '0x' + '01'.repeat(32),
    lockHashPrefix: null,
    address:
      'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqfs6k9ygrwtdrn828l4a04ns9fnes960jg3x8k26',
    roles: [],
    tags: 0,
    lockKind: 'Default Lock',
    ckbDelta: '-13300006408',
    usedDelta: '32400000000',
    itemDeltas: [],
    ...over,
  };
}

const objectArrived: ItemDelta = {
  kind: 'object',
  objectId: '0x8e3821' + '00'.repeat(26) + '7e6f',
  delta: 1,
  collection: { name: '.cell deeds' },
} as ItemDelta;

function activity(participants: ParticipantInfo[]): ActivityRowData {
  return {
    txHash: '0x' + 'cd'.repeat(32),
    blockNumber: 20757774,
    timestamp: '1700000000000',
    isCellbase: false,
    protocolActions: [],
    typeCalls: [],
    lockCalls: [],
    participants,
  };
}

/** Every Tailwind class in the participants grid, the grid itself included. */
function gridClasses(container: HTMLElement): string[] {
  const grid = container.querySelector('[data-role="participants"]') as HTMLElement;
  expect(grid).not.toBeNull();
  return [grid, ...grid.querySelectorAll<HTMLElement>('*')].flatMap((e) =>
    typeof e.className === 'string' ? e.className.split(/\s+/) : []
  );
}

describe('activity participant grid (A1: the stacked form up to 767 px)', () => {
  // At 640 the container is 608 px; a 4- or 5-track grid put the item track at x≈605 and pushed
  // the page 100–150 px sideways (report B#1–5), and its 136 px party track clipped the role to
  // "ow"/"ma" (B#11). The tracks start at md (768), the 390 stacked form serves 640.
  it.each([
    ['the plain grid', false],
    ['the lock-column grid', true],
  ])('%s keeps the stacked form below md and no sm: track or column', (_name, lockColumn) => {
    const { container } = render(
      <ActivityRow
        lockColumn={lockColumn}
        activity={activity([
          party({ roles: ['owner_from'], itemDeltas: [objectArrived] }),
          party({ roles: ['owner_to', 'manager_to'], ckbDelta: '13300000000' }),
        ])}
      />
    );
    const grid = container.querySelector('[data-role="participants"]') as HTMLElement;
    // Below md one participant per line group (A3 made the line a wrapping row); the grid's tracks
    // start at md.
    expect(grid).toHaveClass('flex', 'flex-col', 'md:grid');
    expect(grid.className).toMatch(/(^|\s)md:grid-cols-\[/);
    const classes = gridClasses(container);
    expect(classes.filter((c) => c.startsWith('sm:'))).toEqual([]);
    // The role is whole text in the party cell, not a clipped prefix.
    const parties = [...container.querySelectorAll('[data-party]')];
    expect(parties[0]).toHaveTextContent('owner_from');
    expect(parties[1]).toHaveTextContent('owner_to, manager_to');
  });
});

describe('activity participant line (A3: address and lock kind are one unit)', () => {
  // Report A#15: the lock label wrapped alone or split "Default / Lock" beside a long amount. Below
  // md the party (address, roles, lock kind) is one unit that never breaks; the amount stays at
  // the right end of the line and drops under the party when the line cannot hold both.
  function plainRow(p: ParticipantInfo) {
    const { container } = render(<ActivityRow activity={activity([p])} />);
    const row = container.querySelector('[data-role="participant"]') as HTMLElement;
    expect(row).not.toBeNull();
    return row;
  }

  it('keeps the address and its lock kind together and lets the amount drop', () => {
    const row = plainRow(party({ ckbDelta: '67073483600000000' }));
    expect(row).toHaveClass('flex', 'flex-wrap', 'md:contents');
    const unit = row.querySelector('[data-role="party-unit"]') as HTMLElement;
    expect(unit).toHaveClass('flex', 'md:contents');
    const cell = unit.querySelector('[data-party]') as HTMLElement;
    expect(cell).toHaveTextContent('ckb1qzda...3x8k26Default Lock');
    expect(within(cell).getByText('Default Lock')).toHaveClass('shrink-0', 'whitespace-nowrap');
    const amount = row.querySelector('[data-ckb-amount]')!.parentElement as HTMLElement;
    expect(amount).toHaveClass('ml-auto', 'text-right', 'md:ml-0');
    expect(row.querySelector('[data-role="occupied"]')).toHaveClass(
      'basis-full',
      'whitespace-nowrap',
      'md:basis-auto'
    );
  });

  it('never lets the party cell shrink below its content below md', () => {
    // An overflow-hidden flex item may shrink to nothing and clip the address; the clip is the md
    // grid's 136 px track only.
    const cell = plainRow(party()).querySelector('[data-party]') as HTMLElement;
    expect(cell).not.toHaveClass('min-w-0');
    expect(cell).not.toHaveClass('overflow-hidden');
    expect(cell).toHaveClass('md:min-w-0', 'md:overflow-hidden');
  });

  it('lets the roles wrap under the address instead of being clipped to "ow"', () => {
    // At md and up the party track is 136 px: "ckb1q…nunp36 owner_from" is 185 px (B#11 at 1280).
    const cell = plainRow(party({ roles: ['owner_from'] })).querySelector('[data-party]')!;
    const ref = within(cell as HTMLElement).getByText('owner_from').parentElement as HTMLElement;
    expect(ref).toHaveClass('flex-wrap');
    expect(ref).not.toHaveClass('shrink-0');
  });
});

describe('activity lock-column line below md (A8: the narrow form keeps the lock kind)', () => {
  // Report C#8: the Activities page dropped each participant's lock kind at 390 (its column was
  // `hidden` below the grid breakpoint) while 640 showed it. Below md it is now the tail of the
  // party unit, the same element that is the lock column from md up.
  it('shows the lock kind beside the address below md and in its own column from md', () => {
    const { container } = render(
      <ActivityRow lockColumn activity={activity([party({ lockKind: 'JoyID' })])} />
    );
    const unit = container.querySelector('[data-role="party-unit"]') as HTMLElement;
    const kinds = container.querySelectorAll('[data-role="lock-kind"]');
    expect(kinds).toHaveLength(1);
    const kind = kinds[0] as HTMLElement;
    expect(kind).toHaveTextContent('JoyID');
    expect(kind.parentElement).toBe(unit);
    expect(kind.previousElementSibling).toHaveAttribute('data-party');
    expect(kind).not.toHaveClass('hidden');
    expect(kind).toHaveClass('shrink-0', 'whitespace-nowrap', 'md:col-start-2');
    // Not repeated in the party cell.
    expect(unit.querySelector('[data-party]')).not.toHaveTextContent('JoyID');
  });

  it('wraps a lock kind that cannot follow the address and roles under them, whole', () => {
    // Gate at 390 (cluster): with "owner_to, manager_to" the unit shrank, the roles wrapped inside
    // the party cell, and the lock kind sat at the far end of the line, away from its address.
    // The unit wraps instead: the party cell keeps its width, the lock kind starts the next line.
    const { container } = render(
      <ActivityRow
        lockColumn
        activity={activity([
          party({ roles: ['owner_to', 'manager_to'], lockKind: 'InputTypeProxyLock' }),
        ])}
      />
    );
    const unit = container.querySelector('[data-role="party-unit"]') as HTMLElement;
    expect(unit).toHaveClass('flex-wrap', 'max-w-full');
    expect(unit.querySelector('[data-party]')).toHaveClass('flex-wrap');
    expect(unit.querySelector('[data-role="lock-kind"]')).toHaveClass('whitespace-nowrap');
  });
});
