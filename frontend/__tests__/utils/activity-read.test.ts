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
// a party as the checker states a row's parties from it (lane feed): its lock hash, tags and CKB;
// its tags the route's (a token's bit where it moves an item) unless the test writes them (a
// script call's, C7)
let parties = 0;
const party = (
  itemDeltas: object[] = [],
  tags: number = itemDeltas.length > 0 ? 1 : 0,
  ckbDelta = '0'
) => ({
  lockHash: `0x${String((parties += 1)).padStart(64, '0')}`,
  lockHashPrefix: null,
  tags,
  ckbDelta,
  itemDeltas,
});
/** What an address's row states of every party (its own changes are the row's). */
const addressParties = {
  participantCount: 2,
  partyIds: [['0x' + '01'.repeat(20), 0]],
  feePayer: null,
  items: [],
};

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
    // re-pinned on purpose (lane feed): the checker gives back the row as the route states it (its
    // parties' statements and its items' movement beside what the fixture wrote); it was the row
    expect(activityRead(transfer)).toMatchObject(transfer);
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
    // an address's row: its own items beside the other parties, who carry none on the wire (lane
    // feed: it states what the route states of every party, the parties by their short ids)
    const { participants: _drawn, ...typed } = feedRow({ recognizedTypes: [SUDT] });
    expect(() =>
      activityRead({
        ...typed,
        itemDeltas: [],
        ckbDelta: '0',
        ...addressParties,
        partyIds: [...addressParties.partyIds, ['0x' + '02'.repeat(20), 1]],
        // re-pinned on purpose (lane feed, part 4): the other party's token is in the row's
        // stated items, the transaction's, beside the address's own (none)
        items: [{ ...token('0x71'), movement: 'transfer', amount: '1', parties: 2 }],
      })
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
      {
        ...feedRow({}),
        itemDeltas: [token('0x71')],
        ckbDelta: '0',
        ...addressParties,
      },
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

describe('activityRead states what the route states of every party (lane feed)', () => {
  it('states a row written with every party as the route states it', () => {
    const [other, payer] = [party(), party([], 0, '-1000')];
    const row = activityRead(feedRow({ participants: [other, payer] })) as unknown as {
      participantCount: number;
      partyIds: [string, number][];
      feePayer: string | null;
    };
    expect(row.participantCount).toBe(2);
    // a fixture's party by its whole lock hash (its seeds share their first 20 bytes)
    expect(row.partyIds).toEqual([
      [other.lockHash, 0],
      [payer.lockHash, 0],
    ]);
    expect(row.feePayer).toBe(payer.lockHash);
  });

  it('states each item’s movement in the whole transaction where the fixture writes none', () => {
    const minted = activityRead(
      feedRow({ recognizedTypes: [SUDT], participants: [party([token('0x71')]), party()] })
    ) as unknown as { participants: { itemDeltas: { movement: string }[] }[] };
    expect(minted.participants[0].itemDeltas[0].movement).toBe('mint');
  });

  it('keeps statements a fixture writes, and refuses some of them alone', () => {
    const drawn = [party(), party()];
    const ids = [...drawn, party()].map((p) => [p.lockHash.slice(0, 42), 0]);
    const row = activityRead(
      feedRow({
        participants: drawn,
        participantCount: 3,
        partyIds: ids,
        feePayer: null,
        items: [],
      })
    ) as unknown as { participantCount: number };
    expect(row.participantCount).toBe(3);
    // re-pinned on purpose (lane feed, part 4): the route states four, its items with them
    refuses(
      feedRow({ participantCount: 2 }),
      'states some of participantCount, partyIds, feePayer and items: the route states all four'
    );
    refuses(
      feedRow({ participantCount: 1, partyIds: ids.slice(0, 1), feePayer: null, items: [] }),
      'draws 2 parties and names 1 of 1'
    );
  });

  it('states again a row written over a stated one, from the parties it writes', () => {
    const base = activityRead(feedRow({})) as object;
    const payer = party([], 0, '-5');
    const over = activityRead({ ...base, participants: [payer] }) as unknown as {
      participantCount: number;
      feePayer: string | null;
    };
    expect(over.participantCount).toBe(1);
    expect(over.feePayer).toBe(payer.lockHash);
  });

  it('refuses an address’s row that does not state its parties, or draws other parties', () => {
    const { participants: _drawn, ...address } = feedRow({});
    refuses(
      { ...address, itemDeltas: [], ckbDelta: '0' },
      "is an address's row and states no participantCount, partyIds, feePayer or items"
    );
    refuses(
      { ...feedRow({}), itemDeltas: [], ckbDelta: '0', ...addressParties },
      "is an address's row and draws parties: the route sends none but the address's own changes"
    );
  });
});

function refuses(row: Record<string, unknown>, said: string) {
  expect(() => activityRead(row)).toThrow(said);
}
