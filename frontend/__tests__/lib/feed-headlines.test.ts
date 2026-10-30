import { describe, expect, it } from 'vitest';
import {
  collectionFeedHeadline,
  itemFeedHeadline,
  DOTBIT_ACTION_HEADLINES,
} from '@/lib/feed-headlines';
import type { ActivityProtocolAction, CollectionFeedContext, ItemFeedContext } from '@/lib/api';

const TX = '0x' + 'f9'.repeat(32);
const ctx = (over: Partial<CollectionFeedContext>): CollectionFeedContext => ({
  action: 'mint',
  minted: 0,
  burned: 0,
  transferred: 0,
  updated: 0,
  items: [],
  ...over,
});
const items = (change: CollectionFeedContext['items'][number]['change'], n: number) =>
  Array.from({ length: n }, (_, i) => ({ id: `0x${change}${i}`, change }));

describe('collectionFeedHeadline (OB-C13, OB-C14)', () => {
  it("names a row by what the tx did to THIS collection's items", () => {
    expect(collectionFeedHeadline(ctx({ minted: 1, items: items('minted', 1) }), TX)).toBe(
      'Minted 1'
    );
    // #18,883,769: Nervape transferred 12 and burned 1 — never "Object mint".
    expect(
      collectionFeedHeadline(
        ctx({
          action: 'mixed',
          transferred: 12,
          burned: 1,
          items: [...items('transferred', 12), ...items('burned', 1)],
        }),
        TX
      )
    ).toBe('Transferred 12 · burned 1');
    expect(
      collectionFeedHeadline(ctx({ action: 'update', updated: 1, items: items('updated', 1) }), TX)
    ).toBe('Updated 1');
  });

  it('refuses a context whose action or items disagree with its counts', () => {
    expect(() =>
      collectionFeedHeadline(
        ctx({ action: 'mint', transferred: 2, items: items('transferred', 2) }),
        TX
      )
    ).toThrow(`${TX}: collection action "mint" disagrees with its counts`);
    expect(() => collectionFeedHeadline(ctx({ minted: 2, items: items('minted', 1) }), TX)).toThrow(
      `${TX}: 1 minted item listed for a count of 2`
    );
    expect(() => collectionFeedHeadline(ctx({ action: 'mixed' }), TX)).toThrow(
      `${TX}: collection action "mixed" disagrees with its counts`
    );
  });
});

const itemCtx = (over: Partial<ItemFeedContext>): ItemFeedContext => ({
  itemId: '0x2d2e',
  action: 'update',
  protocolActions: [],
  ringRelink: null,
  actedOn: 'self',
  createdCell: null,
  consumedCell: null,
  ...over,
});
const dotbit = (action: string, accountId = '0xffff'): ActivityProtocolAction => ({
  protocol: 'dotbit',
  protocolName: '.bit',
  action,
  metadata: { accountId },
});
const bit = { kind: 'identity', protocol: 'dotbit' } as const;

describe('itemFeedHeadline · .bit board wording (ID-C1, ID-C4)', () => {
  it.each([
    ['confirm_proposal', 'Registered'],
    ['renew_account', 'Renewed'],
    ['edit_records', 'Records edited'],
    ['edit_manager', 'Manager changed'],
    ['transfer_account', 'Transferred'],
    ['enable_sub_account', 'Sub-accounts enabled'],
    ['recycle_expired_account', 'Recycled'],
  ])('dotbit:%s reads "%s"', (action, text) => {
    expect(DOTBIT_ACTION_HEADLINES[action]).toBe(text);
    expect(
      itemFeedHeadline(
        itemCtx({ protocolActions: [`dotbit:${action}`] }),
        [dotbit(action)],
        TX,
        bit
      )
    ).toEqual({ text, neighbour: false });
  });

  it('joins several own actions in protocol order and keeps unknown ones raw', () => {
    expect(
      itemFeedHeadline(
        itemCtx({ protocolActions: ['dotbit:renew_account', 'dotbit:edit_records'] }),
        [],
        TX,
        bit
      )
    ).toEqual({ text: 'Renewed · records edited', neighbour: false });
    expect(
      itemFeedHeadline(
        itemCtx({
          protocolActions: ['dotbit:renew_account', 'dotbit:lock_account_for_cross_chain'],
        }),
        [],
        TX,
        bit
      )
    ).toEqual({ text: 'Renewed', raw: ['dotbit:lock_account_for_cross_chain'], neighbour: false });
  });

  it("names a neighbour row by the tx's .bit action and dims it", () => {
    expect(
      itemFeedHeadline(
        itemCtx({ actedOn: 'neighbour' }),
        [dotbit('recycle_expired_account', '0xdaemon')],
        TX,
        bit
      )
    ).toEqual({ text: 'Neighbour recycled', neighbour: true });
    expect(() => itemFeedHeadline(itemCtx({ actedOn: 'neighbour' }), [], TX, bit)).toThrow(
      `${TX}: neighbour .bit row has no .bit action in its transaction`
    );
  });

  it('refuses a row without the API actedOn statement', () => {
    expect(() => itemFeedHeadline(itemCtx({ actedOn: undefined as never }), [], TX, bit)).toThrow(
      `${TX}: item feed row has no actedOn`
    );
  });
});

describe('itemFeedHeadline · other standards', () => {
  const cell = { kind: 'identity', protocolName: '.cell' } as const;
  it('keeps the .cell mapping and dims exactly the rows the API says a neighbour acted on', () => {
    expect(itemFeedHeadline(itemCtx({ protocolActions: ['edit_records'] }), [], TX, cell)).toEqual({
      text: '.cell · edit records',
      neighbour: false,
    });
    expect(
      itemFeedHeadline(
        itemCtx({ actedOn: 'neighbour', ringRelink: { by: 'recycle', byId: '0xcc' } }),
        [],
        TX,
        cell
      )
    ).toEqual({ text: 'Neighbour recycled', neighbour: true });
  });
  it('names the rest by the item action in sentence case (OB-C14)', () => {
    expect(itemFeedHeadline(itemCtx({ action: 'transfer' }), [], TX, { kind: 'object' }).text).toBe(
      'Object transfer'
    );
    expect(itemFeedHeadline(itemCtx({ action: 'renew' }), [], TX, { kind: 'identity' }).text).toBe(
      'Identity renew'
    );
  });
});

// F2 identities (round 2): did:ckb and .bit Cell rows carry no protocol action, only the API's
// coarse item action; each standard words it as its board does (ID-C1). "Handle changed" and
// "Updated by its owner" need an API fact and are not inferred here.
describe('identity standards word their coarse item action (ID-C1)', () => {
  const self = (action: string): ItemFeedContext => ({
    itemId: '0x' + '11'.repeat(32),
    action,
    protocolActions: [],
    ringRelink: null,
    actedOn: 'self',
    createdCell: null,
    consumedCell: null,
  });
  it('names did:ckb and .bit Cell rows in their own words', async () => {
    const { IDENTITY_ACTION_WORDS } = await import('@/lib/feed-headlines');
    const did = { kind: 'identity' as const, actionWords: IDENTITY_ACTION_WORDS.did_ckb };
    const bitCell = { kind: 'identity' as const, actionWords: IDENTITY_ACTION_WORDS.bit_cell };
    expect(itemFeedHeadline(self('mint'), null, TX, did).text).toBe('Created');
    expect(itemFeedHeadline(self('update'), null, TX, did).text).toBe('Document updated');
    expect(itemFeedHeadline(self('mint'), null, TX, bitCell).text).toBe('Minted as a .bit Cell');
    expect(itemFeedHeadline(self('update'), null, TX, bitCell).text).toBe('Updated');
    // An action the standard has no word for keeps the generic, exact name.
    expect(itemFeedHeadline(self('renew'), null, TX, did).text).toBe('Identity renew');
  });
});
