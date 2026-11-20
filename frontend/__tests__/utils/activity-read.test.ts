import { describe, expect, it } from 'vitest';
import { activityRead as checked } from '@/__tests__/utils/activity-read';

/** The checker over this file's partial rows, the shapes its rules read. */
const activityRead = (row: object) => checked(row as never);

// The checker's own tests (the third read's T SHOULD-1, restated for C7 at the merge into
// resync/type-codes): rows the route sends pass, and each rule of the route and the indexer's
// builder (a row's `recognizedTypes`, written as `recognized_type_codes`) refuses the row that
// breaks it. The rows here are its inputs on purpose; the gate exempts this file.

const SUDT = {
  typeCodeHash: '0x5e7a36a77e68eecc013dfa2fe6a23f3b6c344b04005808694ae6dd45eea4cfd5',
  typeHashType: 'type',
  scriptName: 'Simple UDT',
  scriptNameBy: null,
};
const SPORE = {
  typeCodeHash: '0x4a4dce1df3dffff7f8b2cd7dff7303df3b6150c9788cb75dcf6747247132b9f5',
  typeHashType: 'data1',
  scriptName: 'Spore',
  scriptNameBy: null,
};
const DAO = {
  typeCodeHash: '0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e',
  typeHashType: 'type',
  scriptName: 'Nervos DAO',
  scriptNameBy: null,
};
/** The indexer's TAG_TYPE_CALL, bit 6. */
const TYPE_CALL = 64;
const token = (id: string) => ({ kind: 'token', typeScriptHash: id, delta: '1' });
const object = (id: string) => ({ kind: 'object', objectId: id, delta: 1 });
const party = (itemDeltas: object[] = [], tags = 0) => ({ itemDeltas, tags });

/** A feed's row: every party's items and tags. */
const feedRow = (over: Record<string, unknown>) => ({
  txHash: '0xaa',
  blockNumber: 7,
  protocolActions: [] as { protocol: string }[],
  recognizedTypes: [] as object[] | null,
  participants: [party(), party()],
  ...over,
});

describe('activityRead passes the rows the route sends', () => {
  it('passes a transfer that names its token’s code, a CKB transfer with none, a DAO step with the DAO’s', () => {
    const transfer = feedRow({
      recognizedTypes: [SUDT],
      participants: [party([token('0x71')]), party()],
    });
    expect(activityRead(transfer)).toBe(transfer);
    expect(() => activityRead(feedRow({}))).not.toThrow();
    expect(() =>
      activityRead(feedRow({ protocolActions: [{ protocol: 'dao' }], recognizedTypes: [DAO] }))
    ).not.toThrow();
    // two items of one code name it once; a Spore and a token, in the route's order
    expect(() =>
      activityRead(
        feedRow({ recognizedTypes: [SUDT], participants: [party([token('0x71'), token('0x72')])] })
      )
    ).not.toThrow();
    expect(() =>
      activityRead(
        feedRow({
          recognizedTypes: [SPORE, SUDT],
          participants: [party([token('0x71')]), party([object('0x41')])],
        })
      )
    ).not.toThrow();
  });

  it('passes more codes than items moved, and a recognised script that moved nothing, a party a script call', () => {
    // a Spore transfer whose transaction also ran the sUDT (a wallet merging its own token cells)
    expect(() =>
      activityRead(
        feedRow({
          recognizedTypes: [SPORE, SUDT],
          participants: [party([object('0x41')]), party([], TYPE_CALL)],
        })
      )
    ).not.toThrow();
    // the merge alone: no item moved, its party a script call
    expect(() =>
      activityRead(feedRow({ recognizedTypes: [SUDT], participants: [party([], TYPE_CALL)] }))
    ).not.toThrow();
  });

  it('passes a pool row with its list, an address row whose items are the others’', () => {
    expect(() =>
      activityRead(
        feedRow({
          blockNumber: null,
          recognizedTypes: [SUDT],
          participants: [party([token('0x71')])],
        })
      )
    ).not.toThrow();
    expect(() => activityRead(feedRow({ blockNumber: null }))).not.toThrow();
    // an address's row: its own items beside the other parties, who carry none on the wire
    expect(() =>
      activityRead({ ...feedRow({ recognizedTypes: [SUDT] }), itemDeltas: [], participants: [{}] })
    ).not.toThrow();
  });
});

describe('activityRead refuses a row the route cannot send', () => {
  const refuses = (row: Record<string, unknown>, said: string) =>
    expect(() => activityRead(row)).toThrow(said);

  it('refuses a row that states no list: no field, null, or the retired item types', () => {
    const { recognizedTypes: _t, ...noList } = feedRow({});
    refuses(noList, 'states recognizedTypes undefined: the route states a list on every row');
    refuses(
      feedRow({ recognizedTypes: null }),
      'states recognizedTypes null: the route states a list on every row'
    );
    // a pool row too: it states the list, never null
    refuses(feedRow({ blockNumber: null, recognizedTypes: null }), 'states recognizedTypes null');
  });

  it('refuses a row that moves an item and names no code, the fixtures’ 45', () => {
    refuses(
      feedRow({ participants: [party([token('0x71')]), party()] }),
      'activityRead: transaction 0xaa moves 1 item(s) and names no code they carry'
    );
    // an address's own item too
    refuses(
      { ...feedRow({}), itemDeltas: [token('0x71')], participants: [{}] },
      'moves 1 item(s) and names no code they carry'
    );
  });

  it('refuses a DAO step without the DAO’s code, and the DAO’s code without a step', () => {
    refuses(
      feedRow({ protocolActions: [{ protocol: 'dao' }] }),
      'takes a Nervos DAO step and does not name the DAO’s code'
    );
    refuses(
      feedRow({ recognizedTypes: [DAO] }),
      'names the DAO’s code and takes no Nervos DAO step'
    );
  });

  it('refuses a code twice and codes out of the route’s order', () => {
    const one = [party([token('0x71'), object('0x41')])];
    refuses(feedRow({ recognizedTypes: [SUDT, SUDT], participants: one }), 'names the code');
    refuses(
      feedRow({ recognizedTypes: [SUDT, SPORE], participants: one }),
      'the route lists the codes in their bytes’ order'
    );
  });

  it('refuses a recognised code that moved nothing on a row with no party a script call', () => {
    refuses(
      feedRow({ recognizedTypes: [SUDT] }),
      'names 1 recognised code(s) that moved no item and no party is a script call (TAG_TYPE_CALL)'
    );
  });
});
