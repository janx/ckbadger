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
 *
 * And what the route states of every party (lane feed, ruling 14 of gate P; `activity_parties` in
 * crates/api/src/routes/activities.rs), which a page reads and never derives: a row a fixture
 * writes with EVERY party in full and no statement is stated as the route states such a row,
 * its `participantCount` their count, its `partyIds` each party's short id (the first 20 bytes of
 * its lock hash, else the prefix it was named by) with its tags, its `feePayer` by the route's
 * rule; and every item of a party a row draws states its `movement` in the whole transaction where
 * the fixture writes none, the route's rule over the row's items (a row holds every item delta of
 * its transaction). A fixture that writes a statement writes all three, and they agree with its
 * parties; an address's row (no `participants`: the route sends no other party in full) writes
 * them, since its own party's lock hash is the page's, not the row's.
 */

import type { ActivityRowData } from '@/components/activity/activity-row';
import type {
  Activity,
  GlobalActivity,
  ItemDelta,
  ItemMovement,
  ParticipantInfo,
  PartyId,
} from '@/lib/api';

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

/** What the route states of every party of an activity (lane feed). */
type StatementKey = 'participantCount' | 'partyIds' | 'feePayer';
type Statements = Pick<GlobalActivity, StatementKey>;

/**
 * A party as a fixture writes it: every field the route sends, its items' movement the route's
 * rule where it writes none (the checker states it).
 */
export type WrittenParty = Omit<ParticipantInfo, 'itemDeltas'> & {
  itemDeltas: (ItemDelta & { movement?: ItemMovement })[];
};

/** A row as a fixture writes it: the route's statements its own, or left to the checker. */
export type Written<R> = Omit<R, StatementKey | 'participants'> &
  Partial<Statements> &
  (R extends { participants: ParticipantInfo[] } ? { participants: WrittenParty[] } : unknown);

type WrittenRow = Written<GlobalActivity> | Written<Activity> | Written<ActivityRowData>;

/**
 * The row, checked, and stated as the route states it (see the module's head). Typed by the
 * API's rows, a feed's, an address's, or the row component's, as a fixture writes them
 * (`Written`), so a literal written inside keeps its literal fields (an item's `kind`) and its own
 * fields beyond the row's (an item feed's context) as its type wants them; the checker reads it
 * through `RowLike`. Where the literal's own type cannot be inferred, the caller names it:
 * `activityRead<Written<GlobalActivity>>(…)`. What it gives back carries the statements and its
 * parties as the route sends them (an address's row has none: it is typed with them for the
 * row types that do).
 */
export function activityRead<R extends WrittenRow>(
  checked: R
): R & Statements & { participants: ParticipantInfo[] } {
  checkItemTypes(checked as RowLike);
  return stateParties(checked) as R & Statements & { participants: ParticipantInfo[] };
}

/** The item types a row states, checked by the route's rule (see the module's head). */
function checkItemTypes(row: RowLike): void {
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
    return;
  }
  if (row.itemTypes === null) {
    if (typeof row.itemTypesUnread !== 'string' || row.itemTypesUnread.trim() === '')
      throw new Error(`${where} states itemTypes null and no reason: the route says why`);
    if (whole && items.size === 0)
      throw new Error(`${where} reads no item and says its type is unread: only an item’s can be`);
    return;
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
  return;
}

/**
 * A fixture's party's id, as the route names a party by its short id (the first 20 bytes of its
 * lock hash, else the prefix a protocol named it by): its whole lock hash, else its prefix. A
 * fixture's lock hashes are seeds padded with zeros, which share their first 20 bytes, so the
 * route's 20 bytes would name every one of them alike; the page compares ids and reads nothing
 * else of them (an id names a lock whose hash it begins, `namesLock`), and the route's own form is
 * pinned by the API's tests (crates/api/tests/api_activities.rs).
 */
export function partyShortId(
  party: { lockHash: string | null; lockHashPrefix: string | null },
  where: string
): string {
  const id = party.lockHash ?? party.lockHashPrefix;
  if (typeof id !== 'string') throw new Error(`${where}: a party with no lock hash and no prefix`);
  return id;
}

/** The same item, by its kind and id (and, for an identity, its standard): the route's identity. */
function sameItem(a: ItemDelta, b: ItemDelta): boolean {
  if (a.kind === 'token') return b.kind === 'token' && a.typeScriptHash === b.typeScriptHash;
  if (a.kind === 'object') return b.kind === 'object' && a.objectId === b.objectId;
  return b.kind === 'identity' && a.identityId === b.identityId && a.standard === b.standard;
}

/** An item's movement over the row's items, which are every item delta of its transaction. */
function movementOf(parties: readonly WrittenParty[], item: ItemDelta): ItemMovement {
  let net = BigInt(0);
  for (const p of parties)
    for (const i of p.itemDeltas) if (sameItem(i, item)) net += BigInt(i.delta);
  return net > BigInt(0) ? 'mint' : net < BigInt(0) ? 'burn' : 'transfer';
}

/**
 * A row's parties with each item's movement in the whole transaction where the fixture writes
 * none, the route's rule over the parties given, which must be every party that moved an item (a
 * row holds every item delta of its transaction). For a test whose input is its pick of a row.
 */
export function withRouteMovements(
  parties: readonly WrittenParty[],
  anew = false
): ParticipantInfo[] {
  return parties.map((p) => ({
    ...p,
    itemDeltas: p.itemDeltas.map((i) => ({
      ...i,
      movement: (anew ? undefined : i.movement) ?? movementOf(parties, i),
    })),
  }));
}

/**
 * The route's fee payer of a row that holds every party (`fee_payer`): not a cellbase, no Nervos
 * DAO withdrawal completed, no item moved, and exactly one party's CKB changed, down. A fixture
 * that writes a CKB change no route sends (not an integer, to see a page refuse it) names none:
 * no party's change can be read, so none is the fee.
 */
function feePayerOf(
  row: { isCellbase?: boolean; protocolActions?: readonly { protocol: string; action: string }[] },
  parties: readonly WrittenParty[],
  where: string
): string | null {
  if (row.isCellbase) return null;
  if (
    (row.protocolActions ?? []).some(
      (a) => a.protocol === 'dao' && a.action === 'withdraw_complete'
    )
  )
    return null;
  let payer: WrittenParty | null = null;
  for (const p of parties) {
    if (p.itemDeltas.length > 0) return null;
    if (!/^-?\d+$/.test(p.ckbDelta)) return null;
    const delta = BigInt(p.ckbDelta);
    if (delta === BigInt(0)) continue;
    if (delta > BigInt(0) || payer !== null) return null;
    payer = p;
  }
  if (payer === null) return null;
  if (payer.lockHash === null) throw new Error(`${where}: a fee payer named by a prefix alone`);
  return payer.lockHash;
}

/**
 * The statements the checker stated for a row that writes every party, by the id list it made: a
 * fixture that spreads such a row and writes other parties over it carries them stale (and its
 * items' movements), and is stated again from its own parties (what it writes anew of the
 * statements stands).
 */
const stated = new WeakMap<object, Pick<Statements, 'participantCount' | 'feePayer'>>();

/** The row with what the route states of every party (see the module's head). */
function stateParties(written: WrittenRow): unknown {
  const row = written as Partial<Statements> & {
    txHash?: string;
    ckbDelta?: string;
    isCellbase?: boolean;
    protocolActions?: readonly { protocol: string; action: string }[];
    participants?: WrittenParty[];
  };
  const where = `activityRead: transaction ${row.txHash ?? '(no hash)'}`;
  const written3 = [row.participantCount, row.partyIds, row.feePayer].filter(
    (s) => s !== undefined
  );
  if (written3.length !== 0 && written3.length !== 3)
    throw new Error(
      `${where} states some of participantCount, partyIds and feePayer: the route states all three`
    );
  if (row.ckbDelta !== undefined) {
    // an address's row: its own changes are the row's; its own party's id is the page's, so the
    // fixture states what the route states of every party; it sends no other party in full
    if (row.participants !== undefined)
      throw new Error(
        `${where} is an address's row and draws parties: the route sends none but the address's own changes`
      );
    if (written3.length === 0)
      throw new Error(
        `${where} is an address's row and states no participantCount, partyIds or feePayer`
      );
    if (row.partyIds!.length !== row.participantCount)
      throw new Error(`${where} names ${row.partyIds!.length} parties of ${row.participantCount}`);
    return written;
  }
  const parties = row.participants;
  if (parties === undefined) throw new Error(`${where} is no address's row and draws no party`);
  const spread = row.partyIds === undefined ? undefined : stated.get(row.partyIds);
  // a row written over a stated one carries its parties' movements stale too: stated again
  const drawn = withRouteMovements(parties, spread !== undefined);
  if (written3.length === 3 && spread === undefined) {
    const ids = row.partyIds as PartyId[];
    if (ids.length !== row.participantCount || drawn.length > ids.length)
      throw new Error(
        `${where} draws ${drawn.length} parties and names ${ids.length} of ${row.participantCount}`
      );
    return Object.assign({}, written, { participants: drawn });
  }
  const partyIds = parties.map((p): PartyId => [partyShortId(p, where), p.tags]);
  const own = {
    participantCount: parties.length,
    feePayer: feePayerOf(row, parties, where),
  };
  stated.set(partyIds, own);
  // the checked row with its statements: the checker's own output, not a fixture's row
  return Object.assign({}, written, {
    participants: drawn,
    partyIds,
    // over a spread row, a statement it writes anew stands
    participantCount:
      spread && row.participantCount !== spread.participantCount
        ? row.participantCount!
        : own.participantCount,
    feePayer: spread && row.feePayer !== spread.feePayer ? row.feePayer! : own.feePayer,
  });
}

/**
 * A row the route cannot send, written on purpose to see a page refuse it (a committed row whose
 * item types are null with no reason). It checks nothing; the gate pins each use by file (`UNSENT`
 * in __tests__/lib/activity-fixtures-gate.test.ts), so it cannot stand in for the checker.
 */
export function unsentRow<R extends GlobalActivity | Activity | ActivityRowData>(row: R): R {
  return row;
}
