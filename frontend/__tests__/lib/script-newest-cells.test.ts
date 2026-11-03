import type { Cell } from '@/lib/api';
import { NEWEST_CELLS_COUNT, mergeNewestFirst, newestCellsKind } from '@/lib/script-newest-cells';

const cell = (tx: string, index: number, block: number, kind: 'lock' | 'type' = 'lock'): Cell => ({
  txHash: `0x${tx.repeat(64 / tx.length)}`,
  outputIndex: index,
  capacity: '6100000000',
  commonKnowledgeSize: '6100000000',
  lockScriptHash: '0x01',
  dataSize: 0,
  createdAtBlock: block,
  matchedScriptKind: kind,
});
const key = (c: Cell) => `${c.createdAtBlock}/${c.txHash.slice(2, 4)}:${c.outputIndex}`;

describe('newestCellsKind (lane S, ruling 2: one kind per list)', () => {
  it('reads a lock script by its lock index and a type script by its type index', () => {
    expect(newestCellsKind('lock')).toBe('lock');
    expect(newestCellsKind('type')).toBe('type');
  });
  it('reads a lock-and-type family by its lock index (the role its cells list reads first)', () => {
    expect(newestCellsKind('lock+type')).toBe('lock');
  });
  it('has no kind for a family without a registered role', () => {
    expect(newestCellsKind(null)).toBeNull();
    expect(newestCellsKind('')).toBeNull();
  });
});

describe('mergeNewestFirst', () => {
  it('lists five', () => expect(NEWEST_CELLS_COUNT).toBe(5));

  it('merges the references’ newest-first pages on the server’s key order', () => {
    const a = [cell('aa', 1, 300), cell('aa', 0, 300), cell('11', 0, 120)];
    const b = [cell('cc', 0, 300), cell('bb', 2, 299), cell('bb', 0, 200)];
    const merged = mergeNewestFirst(
      'lock',
      [
        { reference: 'ref-a', cells: a },
        { reference: 'ref-b', cells: b },
      ],
      5
    );
    // block desc, then tx hash desc, then output index desc — the index key read backwards.
    expect(merged.map(key)).toEqual(['300/cc:0', '300/aa:1', '300/aa:0', '299/bb:2', '200/bb:0']);
  });

  it('keeps fewer when the references hold fewer', () => {
    const merged = mergeNewestFirst(
      'type',
      [{ reference: 'r', cells: [cell('aa', 0, 9, 'type')] }],
      5
    );
    expect(merged.map(key)).toEqual(['9/aa:0']);
    expect(mergeNewestFirst('type', [], 5)).toEqual([]);
  });

  it('throws when a page is not newest block first (an API that ignores order=desc)', () => {
    // What :8101 before lane S answers: the oldest genesis cells, ascending.
    const ascending = [cell('e2', 7, 0), cell('e2', 30, 0), cell('e2', 54, 0)];
    expect(() =>
      mergeNewestFirst('lock', [{ reference: '0x9bd7…cce8 type', cells: ascending }], 5)
    ).toThrow(
      /0x9bd7…cce8 type: cells\/by-script order=desc listed 0xe2e2.*:7 \(#0\) before 0xe2e2.*:30 \(#0\)/
    );
    expect(() =>
      mergeNewestFirst('lock', [{ reference: 'r', cells: [cell('aa', 0, 5), cell('bb', 0, 6)] }], 5)
    ).toThrow(/not newest block first/);
  });

  it('throws on a row from the other index or a cell two references both list', () => {
    expect(() =>
      mergeNewestFirst('lock', [{ reference: 'r', cells: [cell('aa', 0, 5, 'type')] }], 5)
    ).toThrow(/r: cells\/by-script script_kind=lock returned a type-index row 0xaaaa.*:0/);
    expect(() =>
      mergeNewestFirst(
        'lock',
        [
          { reference: 'r1', cells: [cell('aa', 0, 5)] },
          { reference: 'r2', cells: [cell('aa', 0, 5)] },
        ],
        5
      )
    ).toThrow(/0xaaaa.*:0 is listed by both r1 and r2/);
  });
});
