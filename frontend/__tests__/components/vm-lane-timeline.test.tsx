import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, within } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { VmLaneTimeline } from '@/components/hardforks/vm-lane-timeline';
import producer from '@/__tests__/fixtures/network-api-producer/hardforks-deployments.json';
import type { HardforkHashTypeLaneF2 } from '@/lib/api';

// F2 charts group (CH-40, CH-41): the Hardforks board's VM lanes.

const editions = [
  { shortName: 'Mirana', activationEpoch: 5414, activationDate: '2022-05-10' },
  { shortName: 'Meepo', activationEpoch: 12293, activationDate: '2025-07-01' },
] as const;
const lanes: HardforkHashTypeLaneF2[] = [
  { hashType: 'type', knownVersions: 59, liveReferences: '1830780' },
  { hashType: 'data1', knownVersions: 19, liveReferences: '98551' },
  { hashType: 'data2', knownVersions: 0, liveReferences: '0' },
  { hashType: 'data', knownVersions: 1, liveReferences: '472' },
];
const genesisMs = Date.parse('2019-11-15T21:49:50.812Z');
const tipMs = Date.parse('2026-10-30T00:00:00.000Z');
// R2b-b: each version carries the hash types it is registered under.
const unregistered = producer.scriptDeployments.map((d) => ({ ...d, hashTypes: [] }));
const base = producer.scriptDeployments[0];
const dep = (
  name: string,
  hash: string,
  iso: string,
  hashTypes: ('type' | 'data1' | 'data')[]
) => ({
  ...base,
  versionName: name,
  familyName: name,
  versionHash: hash,
  timestampMs: Date.parse(iso),
  hashTypes,
});
const laned = [
  dep('Default Lock', `0x${'a'.repeat(64)}`, '2019-11-16T00:00:00Z', ['type']),
  dep('xUDT', `0x${'b'.repeat(64)}`, '2024-01-08T00:00:00Z', ['data1']),
  dep('Omni Lock', `0x${'c'.repeat(64)}`, '2023-03-01T00:00:00Z', ['type', 'data1']),
  dep('Old Lock', `0x${'d'.repeat(64)}`, '2020-02-01T00:00:00Z', ['data']),
  dep('Unregistered', `0x${'e'.repeat(64)}`, '2021-02-01T00:00:00Z', []),
];
const texts = () =>
  [...screen.getByRole('img').querySelectorAll('text')].map((node) => node.textContent);

describe('VmLaneTimeline', () => {
  afterEach(() => vi.restoreAllMocks());

  it('keeps every lane, edition and live count readable at phone width', () => {
    stubChartWidth(358);
    render(
      <VmLaneTimeline
        editions={editions}
        lanes={lanes}
        deployments={unregistered}
        genesisMs={genesisMs}
        tipMs={tipMs}
        tipEpoch={15203}
      />
    );
    const svg = screen.getByRole('img');
    expect(svg).toHaveAttribute('width', '358');
    // Lane names over full-width bars, epoch-only edition labels, numbers in each lane's header.
    for (const label of [
      'type · the newest VM',
      'data1 · pinned to VM1',
      'data2 · pinned to VM2',
      'data · pinned to VM0',
      'epoch 5,414',
      'epoch 12,293',
      '1,830,780 · 59 versions',
    ])
      expect(texts()).toContain(label);
    expect(texts()).not.toContain('epoch 5,414 · 2022-05-10');
    const note = [...svg.querySelectorAll('text')].find(
      (node) => node.textContent === 'no known script yet'
    )!;
    expect(note.getAttribute('text-anchor')).toBe('end');
    // Year labels never touch (visual pass at 390: "2020 2021 2022…" ran together).
    const years = [...svg.querySelectorAll('[data-year]')].map((n) => Number(n.getAttribute('x')));
    expect(years.length).toBeGreaterThan(2);
    for (let i = 1; i < years.length; i++)
      expect(years[i] - years[i - 1]).toBeGreaterThanOrEqual(30);
    // The two editions' sub-labels never touch (390 shot: "epoch 5,414" ran into "epoch 12,293").
    const sub = (text: string) => {
      const n = [...svg.querySelectorAll('text')].find((t) => t.textContent === text)!;
      const x = Number(n.getAttribute('x'));
      const w = text.length * 6.5;
      const x0 = n.getAttribute('text-anchor') === 'end' ? x - w : x;
      return { x0, x1: x0 + w, y: Number(n.getAttribute('y')) };
    };
    const [m, e] = [sub('epoch 5,414'), sub('epoch 12,293')];
    expect(m.y !== e.y || m.x1 + 4 <= e.x0 || e.x1 + 4 <= m.x0).toBe(true);
    expect(texts()).not.toContain('live references · known versions');
    // The last edition's label turns left when it would run off the right edge.
    const meepo = [...svg.querySelectorAll('text')].find((n) => n.textContent === 'epoch 12,293')!;
    expect(
      Number(meepo.getAttribute('x')) + 12 * 6.5 <= 358 ||
        meepo.getAttribute('text-anchor') === 'end'
    ).toBe(true);
    // Versions with no registered reference sit on the strip, sharing the plot's insets.
    const strip = screen.getByRole('list', { name: 'Known script-version deployment timeline' });
    expect(strip).toHaveStyle({ left: '8px', right: '8px' });
    expect(within(strip).getAllByRole('button')).toHaveLength(2);
  });

  it('puts each registered version on the lane of every hash type it uses (CH-40, R2b-b)', () => {
    stubChartWidth(1200);
    render(
      <VmLaneTimeline
        editions={editions}
        lanes={lanes}
        deployments={laned}
        genesisMs={genesisMs}
        tipMs={tipMs}
        tipEpoch={15203}
      />
    );
    const svg = screen.getByRole('img');
    const onLane = (lane: string) =>
      [...svg.querySelectorAll(`[data-lane-dot="${lane}"]`)].map(
        (n) => n.getAttribute('aria-label')!.split(' · ')[0]
      );
    expect(onLane('type')).toEqual(['Default Lock', 'Omni Lock']);
    expect(onLane('data1')).toEqual(['Omni Lock', 'xUDT']);
    expect(onLane('data')).toEqual(['Old Lock']);
    expect(onLane('data2')).toEqual([]);
    // Only the unregistered version stays on the strip below.
    const strip = screen.getByRole('list', { name: 'Known script-version deployment timeline' });
    expect(within(strip).getAllByRole('button')).toHaveLength(1);
    expect(screen.getByText(/no registered reference · 1/)).toBeInTheDocument();
    // A lane dot and a strip dot share one evidence panel.
    fireEvent.focus(svg.querySelector('[data-lane-dot="data1"]')!);
    const evidence = screen.getByRole('complementary', {
      name: 'Selected script deployment evidence',
    });
    expect(evidence).toHaveTextContent('Omni Lock');
    // data2 has no known script: its note sits between the two editions, not across Mirana.
    const note = [...svg.querySelectorAll('text')].find(
      (n) => n.textContent === 'not valid before Meepo'
    )!;
    const mirana = [...svg.querySelectorAll('line')].find(
      (n) => n.getAttribute('data-edition') === 'Mirana'
    )!;
    expect(Number(note.getAttribute('x')) - 70).toBeGreaterThan(Number(mirana.getAttribute('x1')));
  });

  it('refuses lanes out of VM order and hardforks outside the chain span', () => {
    stubChartWidth(1200);
    vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() =>
      render(
        <VmLaneTimeline
          editions={editions}
          lanes={[lanes[1], lanes[0], lanes[2], lanes[3]]}
          deployments={[]}
          genesisMs={genesisMs}
          tipMs={tipMs}
          tipEpoch={15203}
        />
      )
    ).toThrow(/out of VM order/);
    expect(() =>
      render(
        <VmLaneTimeline
          editions={editions}
          lanes={lanes}
          deployments={[]}
          genesisMs={genesisMs}
          tipMs={Date.parse('2024-01-01T00:00:00Z')}
          tipEpoch={9000}
        />
      )
    ).toThrow(/between genesis and the tip/);
  });
});
