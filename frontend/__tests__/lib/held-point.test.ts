import { describe, expect, it } from 'vitest';
import { assertHeldSeries, heldPoint } from '@/lib/held-point';

/**
 * Lane exactness, part B (the third read's S4): a day of a "CKB held" history is read by one
 * reader, under the names every "CKB held" figure of the API takes, and an older API's `used` and
 * `unused` are refused by name.
 */
describe('heldPoint', () => {
  it('reads the three figures the route states, every digit', () => {
    expect(
      heldPoint(
        {
          capacity: '9007199254740993',
          occupiedCapacity: '6100000000',
          freeCapacity: '9007193154740993',
        },
        'x'
      )
    ).toEqual({
      capacity: '9007199254740993',
      occupiedCapacity: '6100000000',
      freeCapacity: '9007193154740993',
    });
  });

  it('refuses an older API’s used and unused by name', () => {
    expect(() => heldPoint({ used: '1', unused: '1', capacity: '2' }, 'Old 2024-01-01')).toThrow(
      'Old 2024-01-01: the API sends used and unused, not occupiedCapacity and freeCapacity: it is older than this page'
    );
  });

  it('names a figure the route did not send, and refuses one that is not an integer', () => {
    expect(() => heldPoint({ capacity: '2', occupiedCapacity: '1' }, 'Day')).toThrow(
      // the other readers' words for a field an older API does not send (lib/api-fields.ts)
      'Day: the API sends no freeCapacity: it is older than this page'
    );
    expect(() =>
      heldPoint({ capacity: '2', occupiedCapacity: '1.5', freeCapacity: '0.5' }, 'Day')
    ).toThrow('Day: occupied');
  });
});

describe('assertHeldSeries', () => {
  it('passes the two held bands, and refuses an older API’s bands by name', () => {
    expect(() =>
      assertHeldSeries([{ key: 'occupiedCapacity' }, { key: 'freeCapacity' }], 'H')
    ).not.toThrow();
    expect(() => assertHeldSeries([{ key: 'used' }, { key: 'unused' }], 'H')).toThrow(
      'H: the API sends the bands used and unused, not occupiedCapacity and freeCapacity: it is older than this page'
    );
    expect(() => assertHeldSeries([{ key: 'occupiedCapacity' }], 'H')).toThrow(
      'H: no freeCapacity series'
    );
  });
});
