// Responsive fix plan 2026-11-01, lane A: the activity participant grid across widths. jsdom does
// not lay out, so these assert the structure and the breakpoint classes that decide the layout.
import { render } from '@/__tests__/utils/test-utils';
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
  ])('%s keeps two tracks below md and no sm: track or column', (_name, lockColumn) => {
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
    expect(grid).toHaveClass('grid-cols-[minmax(0,1fr)_max-content]');
    expect(grid.className).toMatch(/(^|\s)md:grid-cols-\[/);
    const classes = gridClasses(container);
    expect(classes.filter((c) => c.startsWith('sm:'))).toEqual([]);
    // The role is whole text in the party cell, not a clipped prefix.
    const parties = [...container.querySelectorAll('[data-party]')];
    expect(parties[0]).toHaveTextContent('owner_from');
    expect(parties[1]).toHaveTextContent('owner_to, manager_to');
  });
});
