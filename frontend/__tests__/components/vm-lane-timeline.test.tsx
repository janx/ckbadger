import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@/__tests__/utils/test-utils';
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
        deployments={producer.scriptDeployments}
        genesisMs={genesisMs}
        tipMs={tipMs}
        tipEpoch={15203}
      />
    );
    const svg = screen.getByRole('img');
    expect(svg).toHaveAttribute('width', '358');
    // Short lane names and epoch-only edition labels where the long forms would collide.
    for (const label of ['type', 'data1', 'data2', 'data', 'epoch 5,414', 'epoch 12,293'])
      expect(texts()).toContain(label);
    expect(texts()).not.toContain('epoch 5,414 · 2022-05-10');
    expect(texts()).toContain('1,830,780');
    const note = [...svg.querySelectorAll('text')].find(
      (node) => node.textContent === 'no known script yet'
    )!;
    expect(note.getAttribute('text-anchor')).toBe('end');
    // The deployment strip shares the lanes' insets, so a dot sits under its date.
    const strip = screen.getByRole('list', { name: 'Known script-version deployment timeline' });
    expect(strip).toHaveStyle({ left: '96px', right: '88px' });
    expect(within(strip).getAllByRole('button')).toHaveLength(2);
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
