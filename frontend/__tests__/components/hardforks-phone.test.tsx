import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@/__tests__/utils/test-utils';
import { stubChartWidth, stubPointerEvents } from '@/__tests__/utils/chart-width';
import { VmLaneTimeline } from '@/components/hardforks/vm-lane-timeline';
import producer from '@/__tests__/fixtures/network-api-producer/hardforks-deployments.json';
import type { HardforkHashTypeLaneF2 } from '@/lib/api';

// PB-02 (UI review 2026-11-14): on a phone the Mirana / Meepo rules ran through every lane's label
// and count, "no known script yet" sat on the dotted lane line, the counts said "1 versions", and
// the version dots were 6–7 px targets that overlapped (tapping is the only way to read one).

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

const editions = [
  { shortName: 'Mirana', activationEpoch: 5414, activationDate: '2022-05-10' },
  { shortName: 'Meepo', activationEpoch: 12293, activationDate: '2025-07-01' },
] as const;
const lanes: HardforkHashTypeLaneF2[] = [
  { hashType: 'type', knownVersions: 60, liveReferences: '1765012' },
  { hashType: 'data1', knownVersions: 19, liveReferences: '97771' },
  { hashType: 'data2', knownVersions: 0, liveReferences: '0' },
  { hashType: 'data', knownVersions: 1, liveReferences: '472' },
];
const genesisMs = Date.parse('2019-11-15T21:49:50.812Z');
const tipMs = Date.parse('2026-10-30T00:00:00.000Z');
const base = producer.scriptDeployments[0];
const dep = (name: string, hex: string, iso: string, hashTypes: ('type' | 'data1' | 'data')[]) => ({
  ...base,
  versionName: name,
  familyName: name,
  versionHash: `0x${hex.repeat(64)}`,
  timestampMs: Date.parse(iso),
  hashTypes,
});
const deployments = [
  dep('Default Lock', 'a', '2019-11-16T00:00:00Z', ['type']),
  dep('Omni Lock', 'c', '2023-03-01T00:00:00Z', ['type', 'data1']),
  dep('xUDT', 'b', '2023-03-02T00:00:00Z', ['data1']),
  dep('Old Lock', 'd', '2020-02-01T00:00:00Z', ['data']),
  dep('Unregistered A', 'e', '2021-02-01T00:00:00Z', []),
  dep('Unregistered B', 'f', '2021-02-02T00:00:00Z', []),
];

function mount(width: number) {
  stubChartWidth(width);
  stubPointerEvents();
  render(
    <VmLaneTimeline
      editions={editions}
      lanes={lanes}
      deployments={deployments}
      genesisMs={genesisMs}
      tipMs={tipMs}
      tipEpoch={15292}
    />
  );
  return screen.getByRole('img');
}
const textNode = (svg: Element, text: string) =>
  [...svg.querySelectorAll('text')].find((t) => t.textContent === text);

describe('the VM lanes on a phone (PB-02)', () => {
  // Re-pinned (readability plan P4: a lane is named in plain words, its hash type second): a
  // lane's header is two lines, its words alone on the first and its hash type with its count on
  // the second (it was one line, "type · the newest VM" beside the count); no rule crosses either.
  it('gives each lane its words, and its hash type and count, lines no edition rule crosses', () => {
    const svg = mount(358);
    const heads = [
      ['follows the newest VM', 'hash_type type', '1,765,012 · 60\u00a0versions'],
      ['pinned to VM1', 'hash_type data1', '97,771 · 19\u00a0versions'],
      ['pinned to VM2', 'hash_type data2', '0 · no known script yet'],
      ['pinned to VM0', 'hash_type data', '472 · 1\u00a0version'],
    ];
    const rules = [...svg.querySelectorAll('line[data-edition]')];
    expect(rules.length).toBeGreaterThan(0);
    for (const [words, label, count] of heads) {
      const head = textNode(svg, words);
      const name = textNode(svg, label);
      const figure = textNode(svg, count);
      expect(head, words).toBeDefined();
      expect(name, label).toBeDefined();
      expect(figure, count).toBeDefined();
      // the hash type and its count on one baseline, under the words
      const baseline = Number(name!.getAttribute('y'));
      expect(Number(figure!.getAttribute('y'))).toBe(baseline);
      expect(Number(head!.getAttribute('y'))).toBeLessThan(baseline);
      // a 12 px line's box: its ascent (11.4) above the baseline, its descent (3.6) under it
      for (const [text, y] of [
        [words, Number(head!.getAttribute('y'))],
        [label, baseline],
      ] as const)
        for (const rule of rules) {
          const [y1, y2] = [Number(rule.getAttribute('y1')), Number(rule.getAttribute('y2'))];
          const crosses = Math.min(y1, y2) < y + 3.6 && Math.max(y1, y2) > y - 11.4;
          expect(crosses, `${rule.getAttribute('data-edition')} crosses "${text}"`).toBe(false);
        }
    }
    // the empty lane says so on its own line, not on its dotted lane line
    expect(textNode(svg, 'no known script yet')).toBeUndefined();
  });

  it('counts one version in the singular', () => {
    const svg = mount(358);
    expect(textNode(svg, '472 · 1\u00a0version')).toBeDefined();
    expect(textNode(svg, '472 · 1\u00a0versions')).toBeUndefined();
  });
});

describe.each([358, 1200])('the version dots at %i px (PB-02)', (width) => {
  it('read by the nearest dot from a hit area at least 24 px tall on every lane', () => {
    const svg = mount(width);
    const readout = screen.getByRole('status', { name: 'Script version readout' });
    for (const lane of ['type', 'data1', 'data']) {
      const hit = svg.querySelector(`[data-dot-hit="${lane}"]`);
      expect(hit, `${lane} has a hit area`).not.toBeNull();
      expect(Number(hit!.getAttribute('height'))).toBeGreaterThanOrEqual(24);
    }
    // a pointer over Omni Lock's dot on the data1 lane (xUDT is a day later, a px away) reads it
    const dot = [...svg.querySelectorAll('[data-lane-dot="data1"] circle')][0];
    const hit = svg.querySelector('[data-dot-hit="data1"]')!;
    fireEvent.pointerMove(hit, {
      clientX: Number(dot.getAttribute('cx')),
      clientY: Number(dot.getAttribute('cy')),
    });
    expect(readout).toHaveTextContent(/^Omni Lock · Omni Lock/);
  });

  it('reads the strip of unregistered versions from a hit area at least 24 px tall', () => {
    mount(width);
    const readout = screen.getByRole('status', { name: 'Script version readout' });
    const hit = document.querySelector('[data-dot-hit="strip"]') as HTMLElement;
    expect(hit).not.toBeNull();
    expect(Number.parseFloat(hit.style.height)).toBeGreaterThanOrEqual(24);
    fireEvent.pointerMove(hit, { clientX: 0, clientY: 0 });
    expect(readout).toHaveTextContent(/^Unregistered A · Unregistered A/);
  });
});
