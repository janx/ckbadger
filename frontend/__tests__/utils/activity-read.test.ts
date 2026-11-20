import { describe, expect, it } from 'vitest';
import { activityRead as checked } from '@/__tests__/utils/activity-read';

/** The checker over this file's partial rows, the shapes its rules read. */
const activityRead = (row: object) => checked(row as never);

// The checker's own tests (the third read's T SHOULD-1): rows the route sends pass, and each rule
// of the route's `item_types` (crates/api/src/routes/activities.rs) refuses the row that breaks it.
// The rows here are its inputs on purpose; the gate exempts this file.

const SUDT = {
  typeCodeHash: '0x5e7a36a77e68eecc013dfa2fe6a23f3b6c344b04005808694ae6dd45eea4cfd5',
  typeHashType: 'type',
  scriptName: 'Simple UDT',
};
const SPORE = {
  typeCodeHash: '0x4a4dce1df3dffff7f8b2cd7dff7303df3b6150c9788cb75dcf6747247132b9f5',
  typeHashType: 'data1',
  scriptName: 'Spore',
};
const DAO = {
  typeCodeHash: '0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e',
  typeHashType: 'type',
  scriptName: 'Nervos DAO',
};
const token = (id: string) => ({ kind: 'token', typeScriptHash: id, delta: '1' });
const object = (id: string) => ({ kind: 'object', objectId: id, delta: 1 });
// a party as the checker states a row's parties from it (lane feed): its lock hash, tags and CKB
let parties = 0;
const party = (itemDeltas: object[] = [], ckbDelta = '0') => ({
  lockHash: `0x${String((parties += 1)).padStart(64, '0')}`,
  lockHashPrefix: null,
  tags: itemDeltas.length > 0 ? 1 : 0,
  ckbDelta,
  itemDeltas,
});
/** What an address's row states of every party (its own changes are the row's). */
const addressParties = {
  participantCount: 2,
  partyIds: [['0x' + '01'.repeat(20), 0]],
  feePayer: null,
};

/** A feed's row: every party's items. */
const feedRow = (over: Record<string, unknown>) => ({
  txHash: '0xaa',
  blockNumber: 7,
  protocolActions: [] as { protocol: string }[],
  itemTypes: [] as object[] | null,
  itemTypesUnread: null as string | null,
  participants: [party(), party()],
  ...over,
});

describe('activityRead passes the rows the route sends', () => {
  it('passes a transfer that names its token’s code, a CKB transfer with none, a DAO step with the DAO’s', () => {
    const transfer = feedRow({
      itemTypes: [SUDT],
      participants: [party([token('0x71')]), party()],
    });
    // re-pinned on purpose (lane feed): the checker gives back the row as the route states it (its
    // parties' statements and its items' movement beside what the fixture wrote); it was the row
    expect(activityRead(transfer)).toMatchObject(transfer);
    expect(() => activityRead(feedRow({}))).not.toThrow();
    expect(() =>
      activityRead(feedRow({ protocolActions: [{ protocol: 'dao' }], itemTypes: [DAO] }))
    ).not.toThrow();
    // two items of one code name it once; a Spore and a token, in the route's order
    expect(() =>
      activityRead(
        feedRow({ itemTypes: [SUDT], participants: [party([token('0x71'), token('0x72')])] })
      )
    ).not.toThrow();
    expect(() =>
      activityRead(
        feedRow({
          itemTypes: [SPORE, SUDT],
          participants: [party([token('0x71')]), party([object('0x41')])],
        })
      )
    ).not.toThrow();
  });

  it('passes an unread row with its reason, a pool row with null and null, an address row whose items are the others’', () => {
    expect(() =>
      activityRead(
        feedRow({
          itemTypes: null,
          itemTypesUnread: 'item 0x41 (Object) of tx 0xaa: no cell recorded',
          participants: [party([object('0x41')])],
        })
      )
    ).not.toThrow();
    expect(() => activityRead(feedRow({ blockNumber: null, itemTypes: null }))).not.toThrow();
    // an address's row: its own items beside the other parties, who carry none on the wire (lane
    // feed: it states what the route states of every party, the parties by their short ids)
    const { participants: _drawn, ...typed } = feedRow({ itemTypes: [SUDT] });
    expect(() =>
      activityRead({
        ...typed,
        itemDeltas: [],
        ckbDelta: '0',
        ...addressParties,
        partyIds: [...addressParties.partyIds, ['0x' + '02'.repeat(20), 1]],
      })
    ).not.toThrow();
  });
});

describe('activityRead refuses a row the route cannot send', () => {
  const refuses = (row: Record<string, unknown>, said: string) =>
    expect(() => activityRead(row)).toThrow(said);

  it('refuses a row that states no itemTypes or no reason field', () => {
    const { itemTypes: _t, ...noTypes } = feedRow({});
    refuses(noTypes, 'states no itemTypes or no itemTypesUnread: the route states both');
    const { itemTypesUnread: _u, ...noReason } = feedRow({});
    refuses(noReason, 'states no itemTypes or no itemTypesUnread');
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
    refuses(feedRow({ itemTypes: [DAO] }), 'names the DAO’s code and takes no Nervos DAO step');
  });

  it('refuses a pool row that says [] where the route sends null', () => {
    refuses(
      feedRow({ blockNumber: null }),
      'is a pool row: the route sends itemTypes null and no reason'
    );
  });

  it('refuses a null without its reason, a reason beside a list, an unread type with no item', () => {
    refuses(
      feedRow({ itemTypes: null }),
      'states itemTypes null and no reason: the route says why'
    );
    refuses(
      feedRow({
        itemTypesUnread: 'why',
        participants: [party([token('0x71')])],
        itemTypes: [SUDT],
      }),
      'lists its item types and gives a reason they are unread'
    );
    refuses(
      feedRow({ itemTypes: null, itemTypesUnread: 'why' }),
      'reads no item and says its type is unread: only an item’s can be'
    );
  });

  it('refuses a code twice, codes out of the route’s order, more codes than items, a code with no item', () => {
    const one = [party([token('0x71'), object('0x41')])];
    refuses(feedRow({ itemTypes: [SUDT, SUDT], participants: one }), 'names the code');
    refuses(
      feedRow({ itemTypes: [SUDT, SPORE], participants: one }),
      'the route lists the codes in their bytes’ order'
    );
    refuses(
      feedRow({ itemTypes: [SPORE, SUDT], participants: [party([token('0x71')])] }),
      'names 2 item code(s) for 1 item(s) moved'
    );
    refuses(feedRow({ itemTypes: [SUDT] }), 'names 1 item code(s) for 0 item(s) moved');
  });
});

describe('activityRead states what the route states of every party (lane feed)', () => {
  it('states a row written with every party as the route states it', () => {
    const [other, payer] = [party(), party([], '-1000')];
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
      feedRow({ itemTypes: [SUDT], participants: [party([token('0x71')]), party()] })
    ) as unknown as { participants: { itemDeltas: { movement: string }[] }[] };
    expect(minted.participants[0].itemDeltas[0].movement).toBe('mint');
  });

  it('keeps statements a fixture writes, and refuses some of them alone', () => {
    const drawn = [party(), party()];
    const ids = [...drawn, party()].map((p) => [p.lockHash.slice(0, 42), 0]);
    const row = activityRead(
      feedRow({ participants: drawn, participantCount: 3, partyIds: ids, feePayer: null })
    ) as unknown as { participantCount: number };
    expect(row.participantCount).toBe(3);
    refuses(
      feedRow({ participantCount: 2 }),
      'states some of participantCount, partyIds and feePayer: the route states all three'
    );
    refuses(
      feedRow({ participantCount: 1, partyIds: ids.slice(0, 1), feePayer: null }),
      'draws 2 parties and names 1 of 1'
    );
  });

  it('states again a row written over a stated one, from the parties it writes', () => {
    const base = activityRead(feedRow({})) as object;
    const payer = party([], '-5');
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
      "is an address's row and states no participantCount, partyIds or feePayer"
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
