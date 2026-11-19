/**
 * An activity row as a test serves it, checked to say what the route says of its item types (the
 * third read's T SHOULD-1: 45 fixtures said `itemTypes: []` on rows that move an item or take a
 * Nervos DAO step, the "unchanged real create_router JSON" among them, and a pool row said `[]`
 * where the route sends `null`; the feeds fold by `itemTypes`, so every page test of a token,
 * object or DAO feed ran on a key the API cannot send). The rule is the route's, read from
 * `item_types` in crates/api/src/routes/activities.rs:
 *
 * - every row states both fields, `itemTypes` and `itemTypesUnread`;
 * - a pool row (no block) states `null` and `null`: the route reads no item's type for the pool;
 * - a committed row whose item the stores cannot name states `null` and the reason, and only an
 *   item's type can be unread, so the row moves one;
 * - otherwise the list, each code (code hash, hash type) once, in the route's order (the code's
 *   bytes, then the hash type's byte), and no reason: the Nervos DAO's code exactly when the row
 *   takes a DAO step, at least one other code when the row moves an item, and, on a row that
 *   states every party's items (a feed's row), no other code when it moves none and no more codes
 *   than the items it moves. An address's row states its own items only (its parties are the
 *   others), so another party's item may name a code on a row that moves none of its own.
 */

import type { ActivityRowData } from '@/components/activity/activity-row';
import type { Activity, GlobalActivity } from '@/lib/api';

/** The Nervos DAO's type code hash (crates/indexer/src/parser/dao.rs `DAO_CODE_HASH`). */
const DAO_CODE_HASH = '0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e';

interface ItemTypeFixture {
  typeCodeHash: string;
  typeHashType: string;
  scriptName: string | null;
}

/**
 * The item types the route names on mainnet, for a fixture to state what the route sends: each as
 * the live capture shows it (fixtures/activities-live-mainnet.json: sUDT, Spore, the .bit account,
 * the DAO) or as the bundled script labels name it (docs/metadata/scripts/xudt.toml, and
 * dotcell-account.toml: a .cell name's type, by its Type ID). A row lists its codes in their bytes'
 * order: Spore 0x4a…, .bit account 0x4f…, xUDT 0x50…, sUDT 0x5e…, the DAO 0x82…, .cell 0xd9….
 */
export const ITEM_TYPE: Readonly<
  Record<'spore' | 'dotbit' | 'xudt' | 'sudt' | 'dao' | 'dotcell', ItemTypeFixture>
> = {
  spore: {
    typeCodeHash: '0x4a4dce1df3dffff7f8b2cd7dff7303df3b6150c9788cb75dcf6747247132b9f5',
    typeHashType: 'data1',
    scriptName: 'Spore',
  },
  dotbit: {
    typeCodeHash: '0x4f170a048198408f4f4d36bdbcddcebe7a0ae85244d3ab08fd40a80cbfc70918',
    typeHashType: 'type',
    scriptName: '.bit Account',
  },
  xudt: {
    typeCodeHash: '0x50bd8d6680b8b9cf98b73f3c08faf8b2a21914311954118ad6609be6e78a1b95',
    typeHashType: 'data1',
    scriptName: 'xUDT',
  },
  sudt: {
    typeCodeHash: '0x5e7a36a77e68eecc013dfa2fe6a23f3b6c344b04005808694ae6dd45eea4cfd5',
    typeHashType: 'type',
    scriptName: 'Simple UDT',
  },
  dao: { typeCodeHash: DAO_CODE_HASH, typeHashType: 'type', scriptName: 'Nervos DAO' },
  dotcell: {
    typeCodeHash: '0xd96cee56727a2bb9a21408c154d278df5095fb4b4dcfd50516156424479bfe54',
    typeHashType: 'type',
    scriptName: 'Cells Account',
  },
};

/** A script's hash type as its byte, the second half of the route's order. */
const HASH_TYPE_BYTE: Readonly<Record<string, number>> = { data: 0, type: 1, data1: 2, data2: 4 };

interface ItemTypeLike {
  typeCodeHash: string;
  typeHashType: string;
}

type ItemLike = { kind: string } & (
  | { typeScriptHash: string }
  | { objectId: string }
  | { identityId: string }
  | Record<string, unknown>
);

interface RowLike {
  txHash?: string;
  blockNumber?: number | null;
  protocolActions?: readonly { protocol: string }[];
  itemTypes?: readonly ItemTypeLike[] | null;
  itemTypesUnread?: string | null;
  /** An address's row: its own items. */
  itemDeltas?: readonly ItemLike[];
  /** A feed's row: every party's items; an address's row: the other parties, without items. */
  participants?: readonly { itemDeltas?: readonly ItemLike[] }[];
}

const itemKey = (item: ItemLike): string => {
  const id =
    'typeScriptHash' in item
      ? item.typeScriptHash
      : 'objectId' in item
        ? item.objectId
        : 'identityId' in item
          ? item.identityId
          : undefined;
  return `${item.kind}:${String(id)}`;
};

/**
 * The row, checked. Typed by the API's rows, a feed's, an address's, or the row component's, so a
 * literal written inside keeps its literal fields (an item's `kind`) and its own fields beyond the
 * row's (an item feed's context) as its type wants them; the checker reads it through `RowLike`.
 * Where the literal's own type cannot be inferred, the caller names it:
 * `activityRead<GlobalActivity>(…)`.
 */
export function activityRead<R extends GlobalActivity | Activity | ActivityRowData>(checked: R): R {
  const row = checked as RowLike;
  const where = `activityRead: transaction ${row.txHash ?? '(no hash)'}`;
  if (row.itemTypes === undefined || row.itemTypesUnread === undefined)
    throw new Error(`${where} states no itemTypes or no itemTypesUnread: the route states both`);
  const parties = row.participants ?? [];
  // a feed's row states every party's items; an address's states its own beside the others
  const whole = parties.length > 0 && parties.every((p) => Array.isArray(p.itemDeltas));
  const items = new Set(
    [...(row.itemDeltas ?? []), ...parties.flatMap((p) => p.itemDeltas ?? [])].map(itemKey)
  );
  if (row.blockNumber === null) {
    if (row.itemTypes !== null || row.itemTypesUnread !== null)
      throw new Error(`${where} is a pool row: the route sends itemTypes null and no reason`);
    return checked;
  }
  if (row.itemTypes === null) {
    if (typeof row.itemTypesUnread !== 'string' || row.itemTypesUnread.trim() === '')
      throw new Error(`${where} states itemTypes null and no reason: the route says why`);
    if (whole && items.size === 0)
      throw new Error(`${where} reads no item and says its type is unread: only an item’s can be`);
    return checked;
  }
  if (row.itemTypesUnread !== null)
    throw new Error(`${where} lists its item types and gives a reason they are unread`);
  const keys = row.itemTypes.map((t) => {
    const byte = HASH_TYPE_BYTE[t.typeHashType];
    if (byte === undefined) throw new Error(`${where}: no hash type "${t.typeHashType}"`);
    return { code: t.typeCodeHash.toLowerCase(), byte };
  });
  for (let i = 1; i < keys.length; i++) {
    const [a, b] = [keys[i - 1], keys[i]];
    if (a.code === b.code && a.byte === b.byte)
      throw new Error(`${where} names the code ${b.code} twice: the route names each code once`);
    if (a.code > b.code || (a.code === b.code && a.byte > b.byte))
      throw new Error(
        `${where} names ${b.code} after ${a.code}: the route lists the codes in their bytes’ order`
      );
  }
  const dao = keys.some((k) => k.code === DAO_CODE_HASH && k.byte === HASH_TYPE_BYTE.type);
  const step = (row.protocolActions ?? []).some((a) => a.protocol === 'dao');
  if (step && !dao)
    throw new Error(`${where} takes a Nervos DAO step and does not name the DAO’s code`);
  if (dao && !step) throw new Error(`${where} names the DAO’s code and takes no Nervos DAO step`);
  const others = keys.length - (dao ? 1 : 0);
  if (items.size > 0 && others === 0)
    throw new Error(`${where} moves ${items.size} item(s) and names no code they carry`);
  if (whole && others > items.size)
    throw new Error(
      `${where} names ${others} item code(s) for ${items.size} item(s) moved: each item carries one`
    );
  return checked;
}

/**
 * A row the route cannot send, written on purpose to see a page refuse it (a committed row whose
 * item types are null with no reason). It checks nothing; the gate pins each use by file (`UNSENT`
 * in __tests__/lib/activity-fixtures-gate.test.ts), so it cannot stand in for the checker.
 */
export function unsentRow<R extends GlobalActivity | Activity | ActivityRowData>(row: R): R {
  return row;
}
