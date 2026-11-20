/**
 * An activity row as a test serves it, checked to say what the route says of the type script codes
 * that ran (the third read's T SHOULD-1, restated for C7 at the merge into resync/type-codes: 45
 * fixtures said `itemTypes: []` on rows that move an item or take a Nervos DAO step; the feeds fold
 * by these codes, so every page test of a token, object or DAO feed ran on a key the API cannot
 * send). The rule is the route's and the indexer's: the route reads `recognizedTypes` from the row
 * (`recognized_types` in crates/api/src/routes/activities.rs), which the indexer's one builder
 * writes (`recognized_type_codes`, crates/indexer/src/db/writer/activities.rs):
 *
 * - every row states `recognizedTypes`, a list, a committed row and a pool row alike (the pool
 *   mirror builds its rows with the same builder);
 * - each code (code hash, hash type) once, in the route's order (the code's bytes, then the hash
 *   type's byte);
 * - the Nervos DAO's code exactly when the row takes a DAO step;
 * - at least one other code when the row moves an item (an item's type is a recognised code that
 *   ran);
 * - on a row that states every party (a feed's row), a recognised code that ran where no item
 *   moved and no DAO step was taken marks a party a script call: some party carries TAG_TYPE_CALL
 *   (the builder sets it on a party whose recognised script moved none of its items: a wallet
 *   merging its own token cells). An address's row states its own items and tags only (its parties
 *   are the others), so it is not held to the parties' rule.
 *
 * Gone with the item-type resolver: `itemTypesUnread` (no row's codes are unread: they are the
 * row's), a pool row's `null`, and "no more codes than items moved" (a recognised script may run
 * and move nothing, and one item's transaction may run several recognised codes).
 */

import type { ActivityRowData } from '@/components/activity/activity-row';
import type { Activity, GlobalActivity } from '@/lib/api';

/** The Nervos DAO's type code hash (crates/indexer/src/parser/dao.rs `DAO_CODE_HASH`). */
const DAO_CODE_HASH = '0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e';

/** The indexer's TAG_TYPE_CALL (crates/ckbadger-store/src/types.rs, bit 6). */
const TAG_TYPE_CALL = 1 << 6;

interface RecognizedTypeFixture {
  typeCodeHash: string;
  typeHashType: string;
  scriptName: string | null;
  scriptNameBy: string | null;
}

/**
 * The recognised codes the route names on mainnet, for a fixture to state what the route sends:
 * each as the live capture shows it (fixtures/activities-live-mainnet.json: sUDT, Spore, the .bit
 * account, the DAO) or as the bundled script labels name it (docs/metadata/scripts/xudt.toml, and
 * dotcell-account.toml: a .cell name's type, by its Type ID), each by its registered reference (no
 * hash type beside the name). A row lists its codes in their bytes' order: Spore 0x4a…, .bit
 * account 0x4f…, xUDT 0x50…, sUDT 0x5e…, the DAO 0x82…, .cell 0xd9….
 */
export const ITEM_TYPE: Readonly<
  Record<'spore' | 'dotbit' | 'xudt' | 'sudt' | 'dao' | 'dotcell', RecognizedTypeFixture>
> = {
  spore: {
    typeCodeHash: '0x4a4dce1df3dffff7f8b2cd7dff7303df3b6150c9788cb75dcf6747247132b9f5',
    typeHashType: 'data1',
    scriptName: 'Spore',
    scriptNameBy: null,
  },
  dotbit: {
    typeCodeHash: '0x4f170a048198408f4f4d36bdbcddcebe7a0ae85244d3ab08fd40a80cbfc70918',
    typeHashType: 'type',
    scriptName: '.bit Account',
    scriptNameBy: null,
  },
  xudt: {
    typeCodeHash: '0x50bd8d6680b8b9cf98b73f3c08faf8b2a21914311954118ad6609be6e78a1b95',
    typeHashType: 'data1',
    scriptName: 'xUDT',
    scriptNameBy: null,
  },
  sudt: {
    typeCodeHash: '0x5e7a36a77e68eecc013dfa2fe6a23f3b6c344b04005808694ae6dd45eea4cfd5',
    typeHashType: 'type',
    scriptName: 'Simple UDT',
    scriptNameBy: null,
  },
  dao: {
    typeCodeHash: DAO_CODE_HASH,
    typeHashType: 'type',
    scriptName: 'Nervos DAO',
    scriptNameBy: null,
  },
  dotcell: {
    typeCodeHash: '0xd96cee56727a2bb9a21408c154d278df5095fb4b4dcfd50516156424479bfe54',
    typeHashType: 'type',
    scriptName: 'Cells Account',
    scriptNameBy: null,
  },
};

/** A script's hash type as its byte, the second half of the route's order. */
const HASH_TYPE_BYTE: Readonly<Record<string, number>> = { data: 0, type: 1, data1: 2, data2: 4 };

interface RecognizedTypeLike {
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
  recognizedTypes?: readonly RecognizedTypeLike[] | null;
  /** An address's row: its own items. */
  itemDeltas?: readonly ItemLike[];
  /** A feed's row: every party's items and tags; an address's row: the other parties, bare. */
  participants?: readonly { itemDeltas?: readonly ItemLike[]; tags?: number }[];
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
  if (!Array.isArray(row.recognizedTypes))
    throw new Error(
      `${where} states recognizedTypes ${String(row.recognizedTypes)}: the route states a list on every row`
    );
  const parties = row.participants ?? [];
  // a feed's row states every party's items; an address's states its own beside the others
  const whole = parties.length > 0 && parties.every((p) => Array.isArray(p.itemDeltas));
  const items = new Set(
    [...(row.itemDeltas ?? []), ...parties.flatMap((p) => p.itemDeltas ?? [])].map(itemKey)
  );
  const keys = row.recognizedTypes.map((t) => {
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
  if (whole && items.size === 0 && !step && others > 0) {
    if (!parties.some((p) => ((p.tags ?? 0) & TAG_TYPE_CALL) !== 0))
      throw new Error(
        `${where} names ${others} recognised code(s) that moved no item and no party is a script call (TAG_TYPE_CALL)`
      );
  }
  return checked;
}

/**
 * A row the route cannot send, written on purpose to see a page refuse it (a row whose recognised
 * codes are no list). It checks nothing; the gate pins each use by file (`UNSENT` in
 * __tests__/lib/activity-fixtures-gate.test.ts), so it cannot stand in for the checker.
 */
export function unsentRow<R extends GlobalActivity | Activity | ActivityRowData>(row: R): R {
  return row;
}
