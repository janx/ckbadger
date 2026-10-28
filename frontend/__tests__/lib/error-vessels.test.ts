import { describe, expect, it } from 'vitest';
import { compareVesselSnapshots, layoutVessels, readVessel } from '@/lib/error-vessels';
import type { VesselSnapshot } from '@/lib/api';
import producer from '@/__tests__/fixtures/vessels-api-producer/vessels-snapshot.json';

const initial = producer as VesselSnapshot;

describe('chain-backed vessel model', () => {
  it('keeps exact shannons and anatomy while converting only diagram coordinates', () => {
    const vessel = readVessel(initial.cells[0], null);
    expect(vessel.cell.capacityShannons).toBe('20000000000');
    expect(vessel.cell.occupiedShannons).toBe('5100000000');
    expect(vessel.fullness).toBe(0.255);
    expect(vessel.entrance).toBe('sampled');
    const large = { ...initial.cells[0], capacityShannons: '90071992547409931234' };
    expect(readVessel(large, null).cell.capacityShannons).toBe(large.capacityShannons);
    expect(() => readVessel({ ...initial.cells[0], occupiedShannons: '5200000000' }, null)).toThrow(
      /Invalid canonical vessel anatomy/
    );
    expect(layoutVessels([vessel], 100, 80).get(vessel.cell.outpoint)).toEqual({
      x: 0,
      y: 0,
      width: 100,
      height: 80,
    });
  });

  it('distinguishes proven consumption from sample rotation and reorg removal', () => {
    const first = initial.cells[0];
    const second = initial.cells[1];
    const next: VesselSnapshot = {
      ...initial,
      anchorBlock: 3,
      cells: [{ ...second, createdAtBlock: 3 }],
      tracked: [
        {
          outpoint: first.outpoint,
          status: 'consumed',
          consumedAtBlock: 3,
          consumingTxHash: '0x' + 'ef'.repeat(32),
        },
        { outpoint: second.outpoint, status: 'live', consumedAtBlock: null, consumingTxHash: null },
      ],
    };
    const delta = compareVesselSnapshots(initial, next);
    expect(delta.consumed).toEqual([first.outpoint]);
    expect(delta.rotatedOrReorganized).toEqual([]);
    expect(
      compareVesselSnapshots(initial, {
        ...next,
        tracked: [{ ...next.tracked[0], status: 'live' }, next.tracked[1]],
      }).rotatedOrReorganized
    ).toEqual([first.outpoint]);
    expect(
      compareVesselSnapshots(initial, {
        ...next,
        tracked: [{ ...next.tracked[0], status: 'notCanonical' }, next.tracked[1]],
      }).consumed
    ).toEqual([]);
    expect(() => compareVesselSnapshots(initial, { ...next, tracked: [] })).toThrow(
      /Missing tracked vessel fate/
    );
  });
});
