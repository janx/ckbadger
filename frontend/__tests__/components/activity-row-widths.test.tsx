// Responsive fix plan 2026-11-01, lane A: the activity participant grid across widths. jsdom does
// not lay out, so these assert the structure and the breakpoint classes that decide the layout.
import { fireEvent, render, within } from '@/__tests__/utils/test-utils';
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

/** Every Tailwind class in the participants box, the box itself included. */
function gridClasses(container: HTMLElement): string[] {
  const grid = container.querySelector('[data-role="participants"]') as HTMLElement;
  expect(grid).not.toBeNull();
  return [grid, ...grid.querySelectorAll<HTMLElement>('*')].flatMap((e) =>
    typeof e.className === 'string' ? e.className.split(/\s+/) : []
  );
}

describe('activity participant lines (A1 → T7: one wrapping line at every width)', () => {
  // A1 kept the 4/5-track grid off the 640 band (it pushed the page 100–150 px sideways, report
  // B#1–5) and the 136 px party track clipped roles to "ow" (B#11). Option A (T7) has no tracks:
  // one wrapping line per participant at every width, so there is no breakpoint to get wrong.
  it.each([
    ['a plain row', false],
    ['a lockColumn row (the prop no longer changes the row)', true],
  ])('%s draws a flex column of wrapping lines, no grid, no sm:/md: track', (_name, lockColumn) => {
    const { container } = render(
      <ActivityRow
        lockColumn={lockColumn}
        activity={activity([
          party({ roles: ['owner_from'], itemDeltas: [objectArrived] }),
          party({ roles: ['owner_to', 'manager_to'], ckbDelta: '13300000000' }),
        ])}
      />
    );
    const box = container.querySelector('[data-role="participants"]') as HTMLElement;
    expect(box).toHaveClass('flex', 'flex-col');
    const classes = gridClasses(container);
    expect(classes.filter((c) => /grid|col-start|col-span|contents/.test(c))).toEqual([]);
    expect(classes.filter((c) => c.startsWith('sm:'))).toEqual([]);
    for (const line of container.querySelectorAll('[data-role="participant"]'))
      expect(line).toHaveClass('flex', 'flex-wrap');
    // The role is whole text in the party cell, not a clipped prefix.
    const parties = [...container.querySelectorAll('[data-party]')];
    expect(parties[0]).toHaveTextContent('owner_from');
    expect(parties[1]).toHaveTextContent('owner_to, manager_to');
  });
});

describe('activity participant line (A3 → T7: party, tag, amount wrap as whole items)', () => {
  // Report A#15: the lock label wrapped alone or split "Default / Lock" beside a long amount. On
  // the T7 line the address, the lock tag and the amount are each one unbreakable item; the line
  // wraps between them, in that order.
  function plainRow(p: ParticipantInfo) {
    const { container } = render(<ActivityRow activity={activity([p])} />);
    const row = container.querySelector('[data-role="participant"]') as HTMLElement;
    expect(row).not.toBeNull();
    return row;
  }

  it('keeps the address, then its lock tag, then the amount, each whole', () => {
    const row = plainRow(party({ ckbDelta: '67073483600000000' }));
    expect(row).toHaveClass('flex', 'flex-wrap');
    expect([...row.children].map((e) => e.getAttribute('data-role'))).toEqual([
      'party',
      'lock-kind',
      'ckb',
      'occupied',
    ]);
    expect(row.querySelector('[data-party]')).toHaveTextContent('ckb1qzda...3x8k26');
    expect(within(row).getByText('Default Lock')).toHaveClass('whitespace-nowrap');
    expect(row.querySelector('[data-ckb-amount]')).toHaveClass('whitespace-nowrap');
    expect(row.querySelector('[data-role="occupied"]')).toHaveClass('whitespace-nowrap');
    expect(row.querySelector('[data-role="occupied"]')).not.toHaveClass('basis-full');
  });

  it('never lets the party cell shrink below its content or clip it', () => {
    // An overflow-hidden flex item may shrink to nothing and clip the address; the party is never
    // clipped, at any width (the md grid's 136 px clip went with the grid).
    const cell = plainRow(party()).querySelector('[data-party]') as HTMLElement;
    expect(cell).not.toHaveClass('min-w-0');
    expect(cell).not.toHaveClass('overflow-hidden');
    expect([...cell.classList].filter((c) => c.startsWith('md:'))).toEqual([]);
  });

  it('lets the roles wrap under the address instead of being clipped to "ow"', () => {
    // At md and up the party track is 136 px: "ckb1q…nunp36 owner_from" is 185 px (B#11 at 1280).
    const cell = plainRow(party({ roles: ['owner_from'] })).querySelector('[data-party]')!;
    const ref = within(cell as HTMLElement).getByText('owner_from').parentElement as HTMLElement;
    expect(ref).toHaveClass('flex-wrap');
    expect(ref).not.toHaveClass('shrink-0');
  });
});

describe('activity lock kind at every width (A8 → T7: the tag is on every line)', () => {
  // Report C#8: the Activities page dropped each participant's lock kind at 390 (its column was
  // `hidden` below the grid breakpoint). On the T7 line the tag is the item right after the
  // party, with no breakpoint class, so no width can hide it.
  // Material pass v2 (move 5): the lock kind is dim small caps after the address, not a chip.
  it('shows the lock kind in small caps right after the address, at every width', () => {
    const { container } = render(
      <ActivityRow lockColumn activity={activity([party({ lockKind: 'JoyID' })])} />
    );
    const kinds = container.querySelectorAll('[data-role="lock-kind"]');
    expect(kinds).toHaveLength(1);
    const kind = kinds[0] as HTMLElement;
    expect(kind).toHaveTextContent('JoyID');
    expect(kind.previousElementSibling).toHaveAttribute('data-party');
    expect([...kind.classList].filter((c) => /hidden|^(sm|md|lg):/.test(c))).toEqual([]);
    expect(kind).toHaveClass('whitespace-nowrap', 'small-caps', 'text-text-dim');
    expect(kind).not.toHaveClass('rounded-[3px]', 'border');
    // Not repeated in the party cell.
    expect(container.querySelector('[data-party]')).not.toHaveTextContent('JoyID');
  });

  it('wraps a lock kind that cannot follow the address and roles to the next line, whole', () => {
    // Gate at 390 (cluster): with "owner_to, manager_to" the unit shrank, the roles wrapped inside
    // the party cell, and the lock kind sat at the far end of the line, away from its address.
    // The tag is its own unbreakable item: when it cannot follow the party, it starts the next
    // line whole; the party cell never exceeds the line.
    const { container } = render(
      <ActivityRow
        lockColumn
        activity={activity([
          party({ roles: ['owner_to', 'manager_to'], lockKind: 'InputTypeProxyLock' }),
        ])}
      />
    );
    const line = container.querySelector('[data-role="participant"]') as HTMLElement;
    expect(line).toHaveClass('flex-wrap');
    expect(line.querySelector('[data-party]')).toHaveClass('max-w-full');
    expect(line.querySelector('[data-role="lock-kind"]')).toHaveClass('whitespace-nowrap');
  });
});

describe('activity detail lines (A12c: repeats collapse, long call runs fold)', () => {
  // Report B#26: a .bit "Registered" entry printed 18 identical ".bit · confirm proposal" lines
  // (the actions differ only in metadata the row does not show) and 29 lock calls one per line at
  // 390 — about 900 px for one entry.
  function dotbitRegistration(): ActivityRowData {
    const confirm = (i: number) => ({
      protocol: 'dotbit',
      protocolName: '.bit',
      action: 'confirm_proposal',
      metadata: { accountId: '0x' + String(i).padStart(40, '0') },
    });
    const lock = (i: number) => ({
      lockCodeHash: '0x' + '93'.repeat(32),
      lockHashType: 'type',
      lockArgs: '0x' + String(i).padStart(4, '0') + 'ab'.repeat(20),
      scriptHash: '0x' + String(i).padStart(64, '0'),
      scriptName: '.bit Lock',
      decoded: null,
      decodeError: null,
    });
    return {
      ...activity([party()]),
      protocolActions: [
        { protocol: 'dotbit', protocolName: '.bit', action: 'register', metadata: {} },
        ...Array.from({ length: 18 }, (_, i) => confirm(i)),
      ],
      lockCalls: Array.from({ length: 29 }, (_, i) => lock(i)),
    } as ActivityRowData;
  }

  it('says a repeated action line once with its count', () => {
    const { container } = render(<ActivityRow activity={dotbitRegistration()} />);
    const lines = [...container.querySelectorAll('[data-role="detail"]')].map((l) => l.textContent);
    expect(lines.filter((t) => t?.includes('confirm proposal'))).toEqual([
      '.bit · confirm proposal ×18',
    ]);
  });

  it('says 29 calls of one lock once, ×29, every call with its args in the hover', () => {
    // T7: names only on the row. The 29 ".bit Lock" calls of report B#26 differ only in args, so
    // the row names the lock once with its count; each call stays whole in the hover.
    const { container } = render(<ActivityRow activity={dotbitRegistration()} />);
    const lockLine = [...container.querySelectorAll('[data-role="detail"]')].find((l) =>
      l.textContent?.startsWith('lock')
    ) as HTMLElement;
    expect(lockLine).toHaveTextContent(/^lock \.bit Lock ×29$/);
    const link = within(lockLine).getByRole('link', { name: '.bit Lock' });
    const calls = link.getAttribute('title')!.split('\n');
    expect(calls).toHaveLength(29);
    expect(calls[0]).toBe(`.bit Lock(0x0000${'ab'.repeat(20)})`);
    expect(calls[28]).toBe(`.bit Lock(0x0028${'ab'.repeat(20)})`);
  });

  it('folds a run of more than six lock names behind "+N more locks" and opens it in place', () => {
    const named = dotbitRegistration();
    const distinct = {
      ...named,
      lockCalls: named.lockCalls.map((l, i) => ({
        ...l,
        lockCodeHash: '0x' + String(i).padStart(64, '0'),
        scriptName: `Lock ${i}`,
      })),
    } as ActivityRowData;
    const { container } = render(<ActivityRow activity={distinct} />);
    const lockLine = () =>
      [...container.querySelectorAll('[data-role="detail"]')].find((l) =>
        l.textContent?.startsWith('lock')
      ) as HTMLElement;
    expect(lockLine().querySelectorAll('a[href*="/script"]')).toHaveLength(5);
    const more = within(lockLine()).getByRole('button', { name: '+24 more locks' });
    expect(more).toHaveAttribute('aria-expanded', 'false');
    fireEvent.click(more);
    expect(lockLine().querySelectorAll('a[href*="/script"]')).toHaveLength(29);
  });
});
