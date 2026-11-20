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
 * - on a row that states every item of its transaction, a recognised code that ran where no item
 *   moved and no DAO step was taken marks a party a script call: some party carries TAG_TYPE_CALL
 *   (the builder sets it on a party whose recognised script moved none of its items: a wallet
 *   merging its own token cells), read from every party's tags as the route names them
 *   (`partyIds`) where the row states them, else from the parties it writes in full.
 *
 * Gone with the item-type resolver: `itemTypes` and `itemTypesUnread` (no row's codes are unread:
 * they are the row's), a pool row's `null`, and "no more codes than items moved" (a recognised
 * script may run and move nothing, and one item's transaction may run several recognised codes).
 *
 * And what the route states of every party (lane feed, ruling 14 of gate P; `activity_parties` in
 * crates/api/src/routes/activities.rs), which a page reads and never derives: a row a fixture
 * writes with EVERY party in full and no statement is stated as the route states such a row,
 * its `participantCount` their count, its `partyIds` each party's id (see `partyShortId`) with its
 * tags, its `feePayer` by the route's rule, its `items` (part 4) by the route's rule over every
 * party; and every item of a party a row draws states its `movement` in the whole transaction where
 * the fixture writes none, the route's rule over the parties written. A fixture that writes a
 * statement writes all four, and they agree with its parties; an address's row (no
 * `participants`: the route sends no other party in full) writes them, since its own party's lock
 * hash is the page's, not the row's.
 *
 * And the senders bit (resync/int, at the merge of lane feed): the route states a party's `sent`
 * and the bit in its tags (TAG_INPUT, bit 8) from one fact, the indexer's tags, in a drawn party's
 * `tags`, in its id's tags (`partyIds`) and in an address row's own `tags`. A fixture writes
 * `sent`; the checker states the bit in those tags (`routeTags`), and refuses a party or a row
 * whose tags carry the bit while it says it did not send, and an id list that names a drawn party
 * by tags that say otherwise than its `sent`.
 */

import type { ActivityRowData } from '@/components/activity/activity-row';
import type {
  Activity,
  GlobalActivity,
  ItemDelta,
  ItemMovement,
  ParticipantInfo,
  PartyId,
  RowItem,
} from '@/lib/api';

/** The Nervos DAO's type code hash (crates/indexer/src/parser/dao.rs `DAO_CODE_HASH`). */
const DAO_CODE_HASH = '0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e';

/** The indexer's TAG_TYPE_CALL (crates/ckbadger-store/src/types.rs, bit 6). */
const TAG_TYPE_CALL = 1 << 6;

/** The indexer's TAG_INPUT, the senders bit (crates/ckbadger-store/src/types.rs, bit 8). */
const TAG_INPUT = 1 << 8;

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
  /** A feed's row: the parties it draws, with their items and tags. */
  participants?: readonly { itemDeltas?: readonly ItemLike[]; tags?: number }[];
  /** The transaction's items, stated once by the route (lane feed, part 4): every one it moves. */
  items?: readonly ItemLike[];
  /** Every party by its short id and tags, stated by the route (lane feed). */
  partyIds?: readonly PartyId[];
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
type StatementKey = 'participantCount' | 'partyIds' | 'feePayer' | 'items';
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
  Partial<Pick<R, StatementKey & keyof R>> &
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
  const parties = stateParties(checked);
  // an address's row: the route sends no other party in full, and the fixture states the rest
  if (parties === null) return checked as R & Statements & { participants: ParticipantInfo[] };
  // the row it checked as the route states it: the checker's output (the gate's reader passes a
  // row returned here, from this body, and nowhere else)
  return { ...checked, ...parties.statements, participants: parties.drawn } as R &
    Statements & { participants: ParticipantInfo[] };
}

/** The item types a row states, checked by the route's rule (see the module's head). */
function checkItemTypes(row: RowLike): void {
  const where = `activityRead: transaction ${row.txHash ?? '(no hash)'}`;
  if (!Array.isArray(row.recognizedTypes))
    throw new Error(
      `${where} states recognizedTypes ${String(row.recognizedTypes)}: the route states a list on every row`
    );
  const parties = row.participants ?? [];
  // the transaction's items: the route's statement of them where the fixture writes it (a row
  // draws no party for moving an item), else the parties it writes, every one of them (a
  // statement the checker made for a row spread here is the spread row's, not this one's)
  const statedItems =
    row.items !== undefined && !checkerItems.has(row.items) ? row.items : undefined;
  const whole =
    statedItems !== undefined ||
    (parties.length > 0 && parties.every((p) => Array.isArray(p.itemDeltas)));
  const items = new Set(
    [
      ...(statedItems ?? []),
      ...(row.itemDeltas ?? []),
      ...parties.flatMap((p) => p.itemDeltas ?? []),
    ].map(itemKey)
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
    // the id list the fixture writes, else its parties' (a list the checker stated for a row
    // spread here is the spread row's, and is stated again from this row's parties)
    const tags =
      row.partyIds !== undefined && !stated.has(row.partyIds)
        ? row.partyIds.map(([, t]) => t)
        : parties.map((p) => p.tags ?? 0);
    if (!tags.some((t) => (t & TAG_TYPE_CALL) !== 0))
      throw new Error(
        `${where} names ${others} recognised code(s) that moved no item and no party is a script call (TAG_TYPE_CALL)`
      );
  }
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

/**
 * A party's tags as the route states them: the senders bit set where it sent (the route states
 * `sent` and the bit from one fact); a party whose tags carry the bit while it says it did not
 * send is refused. A party written without `sent` (an API older than the page, written to see a
 * page refuse it) keeps the tags it writes.
 */
function routeTags(party: { tags: number; sent?: boolean }, where: string, who: string): number {
  if (typeof party.sent !== 'boolean') return party.tags;
  if (!party.sent && (party.tags & TAG_INPUT) !== 0)
    throw new Error(
      `${where}: ${who} carries the senders bit (TAG_INPUT) in its tags and says it did not send: the route states both from one bit`
    );
  return party.sent ? party.tags | TAG_INPUT : party.tags;
}

/** A fixture's party as the route names it in `partyIds`: its id and its tags, senders bit set. */
export function partyId(party: WrittenParty, where: string): PartyId {
  return [
    partyShortId(party, where),
    routeTags(party, where, `party ${partyShortId(party, where)}`),
  ];
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
 * The route's items of a row that holds every party (`ItemsMoved`, lane feed part 4): each item
 * once in the order it first appears, each party's deltas of it netted first; `parties` the parties
 * whose holdings changed; `movement` by what they gained against what they gave; a token's
 * `amount` the larger of the two.
 */
function itemsOf(parties: readonly WrittenParty[]): RowItem[] {
  const totals: { first: ItemDelta; received: bigint; sent: bigint; parties: number }[] = [];
  for (const p of parties) {
    const nets: { first: ItemDelta; net: bigint }[] = [];
    for (const d of p.itemDeltas) {
      const net = nets.find((n) => sameItem(n.first, d));
      if (net) net.net += BigInt(d.delta);
      else nets.push({ first: d, net: BigInt(d.delta) });
    }
    for (const { first, net } of nets) {
      let total = totals.find((t) => sameItem(t.first, first));
      if (!total) {
        total = { first, received: BigInt(0), sent: BigInt(0), parties: 0 };
        totals.push(total);
      }
      if (net === BigInt(0)) continue;
      if (net > BigInt(0)) total.received += net;
      else total.sent -= net;
      total.parties += 1;
    }
  }
  return totals.map(({ first, received, sent, parties }) => {
    const movement: ItemMovement = received > sent ? 'mint' : received < sent ? 'burn' : 'transfer';
    const { delta: _delta, ...named } = first as ItemDelta & { movement?: ItemMovement };
    delete (named as { movement?: ItemMovement }).movement;
    const amount = (received > sent ? received : sent).toString();
    return (
      named.kind === 'token'
        ? { ...named, movement, parties, amount }
        : { ...named, movement, parties }
    ) as RowItem;
  });
}

/**
 * The statements the checker stated for a row that writes every party, by the id list it made: a
 * fixture that spreads such a row and writes other parties over it carries them stale (and its
 * items' movements), and is stated again from its own parties (what it writes anew of the
 * statements stands).
 */
const stated = new WeakMap<object, Omit<Statements, 'partyIds'>>();

/** The item statements the checker made, which a row spread from a stated one carries stale. */
const checkerItems = new WeakSet<object>();

/**
 * What the route states of every party of a row (see the module's head): the parties it draws,
 * their items' movement stated, and its statements; null for an address's row, which states them
 * itself and draws no party.
 */
function stateParties(
  written: WrittenRow
): { drawn: ParticipantInfo[]; statements: Statements } | null {
  const row = written as Partial<Statements> & {
    txHash?: string;
    ckbDelta?: string;
    tags?: number;
    sent?: boolean;
    isCellbase?: boolean;
    protocolActions?: readonly { protocol: string; action: string }[];
    participants?: WrittenParty[];
  };
  const where = `activityRead: transaction ${row.txHash ?? '(no hash)'}`;
  const written4 = [row.participantCount, row.partyIds, row.feePayer, row.items].filter(
    (s) => s !== undefined
  );
  if (written4.length !== 0 && written4.length !== 4)
    throw new Error(
      `${where} states some of participantCount, partyIds, feePayer and items: the route states all four`
    );
  if (row.ckbDelta !== undefined) {
    // an address's row: its own changes are the row's; its own party's id is the page's, so the
    // fixture states what the route states of every party; it sends no other party in full
    if (row.participants !== undefined)
      throw new Error(
        `${where} is an address's row and draws parties: the route sends none but the address's own changes`
      );
    if (written4.length === 0)
      throw new Error(
        `${where} is an address's row and states no participantCount, partyIds, feePayer or items`
      );
    if (row.partyIds!.length !== row.participantCount)
      throw new Error(`${where} names ${row.partyIds!.length} parties of ${row.participantCount}`);
    if (row.tags !== undefined) routeTags({ tags: row.tags, sent: row.sent }, where, 'the address');
    return null;
  }
  const parties = row.participants;
  if (parties === undefined) throw new Error(`${where} is no address's row and draws no party`);
  const spread = row.partyIds === undefined ? undefined : stated.get(row.partyIds);
  // a row written over a stated one carries its parties' movements stale too: stated again; and
  // each drawn party's tags as the route states them, the senders bit where it sent
  const drawn = withRouteMovements(parties, spread !== undefined).map((p) => ({
    ...p,
    tags: routeTags(p, where, `party ${partyShortId(p, where)}`),
  }));
  if (written4.length === 4 && spread === undefined) {
    const ids = row.partyIds as PartyId[];
    if (ids.length !== row.participantCount || drawn.length > ids.length)
      throw new Error(
        `${where} draws ${drawn.length} parties and names ${ids.length} of ${row.participantCount}`
      );
    // the route names a drawn party by the tags it draws it with: the senders bit agrees
    for (const p of drawn) {
      if (typeof p.sent !== 'boolean') continue;
      const id = partyShortId(p, where);
      const named = ids.find(([n]) => n === id || n === id.slice(0, 42));
      if (named === undefined) throw new Error(`${where} draws party ${id} and names it nowhere`);
      if (((named[1] & TAG_INPUT) !== 0) !== p.sent)
        throw new Error(
          `${where} draws party ${id} as ${p.sent ? 'a sender' : 'no sender'} and names it with tags ${named[1]}: the route states both from one bit (TAG_INPUT)`
        );
    }
    return {
      drawn,
      statements: {
        participantCount: row.participantCount!,
        partyIds: ids,
        feePayer: row.feePayer!,
        items: row.items!,
      },
    };
  }
  const partyIds = parties.map((p) => partyId(p, where));
  const own = {
    participantCount: parties.length,
    feePayer: feePayerOf(row, parties, where),
    items: itemsOf(parties),
  };
  stated.set(partyIds, own);
  checkerItems.add(own.items);
  return {
    drawn,
    statements: {
      partyIds,
      // over a spread row, a statement it writes anew stands
      participantCount:
        spread && row.participantCount !== spread.participantCount
          ? row.participantCount!
          : own.participantCount,
      feePayer: spread && row.feePayer !== spread.feePayer ? row.feePayer! : own.feePayer,
      items: spread && row.items !== spread.items ? row.items! : own.items,
    },
  };
}

/**
 * A row the route cannot send, written on purpose to see a page refuse it (a row whose recognised
 * codes are no list). It checks nothing; the gate pins each use by file (`UNSENT` in
 * __tests__/lib/activity-fixtures-gate.test.ts), so it cannot stand in for the checker.
 */
export function unsentRow<R extends GlobalActivity | Activity | ActivityRowData>(row: R): R {
  return row;
}
