/**
 * "What moved" (the transaction page, review 2026-11-03 v2): every asset a transaction moved as
 * one flow from the parties it came from to the parties it went to, read off the activity's
 * signed per-party deltas — the API's Layer-1 opinion, never re-derived from cells. No pairing is
 * invented: a flow has givers and receivers and passes through the asset, which is all the chain
 * states. The CKB flow is conserved on the page: the fee leaves to the miner and Nervos DAO
 * compensation enters, so what left the givers equals what reached the receivers. Every amount is
 * an exact bigint; a sum that does not conserve is an upstream bug and throws with the transaction
 * hash.
 */
import { TAG_DAO, type GlobalActivity, type ItemDelta, type ParticipantInfo } from '@/lib/api';
import { protocolActionMeta } from '@/lib/protocol-action-meta';
import { PARTICIPANT_LIMIT } from '@/lib/participant-order';

export type FlowAsset =
  | { kind: 'ckb' }
  | { kind: 'token'; item: Extract<ItemDelta, { kind: 'token' }> }
  | { kind: 'object'; item: Extract<ItemDelta, { kind: 'object' }> }
  | { kind: 'identity'; item: Extract<ItemDelta, { kind: 'identity' }> };

/**
 * One end of a flow: a party with its exact amount, or a terminal — the miner (the fee, a
 * receiver), Nervos DAO compensation (a giver), an item minted here (no giver on chain) or burnt
 * here (no holder after). `more` is a fold of party ends (see `foldEnds`).
 */
export type FlowEnd =
  | { t: 'party'; party: ParticipantInfo; amount: bigint }
  | { t: 'miner'; amount: bigint }
  | { t: 'compensation'; amount: bigint }
  | { t: 'minted'; amount: bigint }
  | { t: 'burnt'; amount: bigint }
  | { t: 'more'; count: number; amount: bigint; ends: readonly FlowEnd[] };

export interface TxFlow {
  /** `ckb`, `token:<type hash>`, `object:<id>` or `identity:<standard>:<id>`. */
  key: string;
  asset: FlowAsset;
  /** Givers, largest amount first; a giving terminal (compensation, minted) first. */
  from: FlowEnd[];
  /** Receivers, largest amount first; a receiving terminal (the miner, burnt) last. */
  to: FlowEnd[];
  /** What left the givers, terminals included: the flow's volume. */
  total: bigint;
  /**
   * What left the giving parties: `from`'s party ends, without a giving terminal (compensation,
   * minted). The figure of a sentence that says what left them.
   */
  sent: bigint;
  /**
   * What reached the receiving parties: `to`'s party ends, without a receiving terminal (the
   * miner's fee, burnt). The figure of a sentence that says what moved to them (lane P1).
   */
  received: bigint;
}

const ZERO = BigInt(0);

/** A Nervos DAO step in words, one and several, by the activity's action (`dao` actions). */
const DAO_STEP: Readonly<Record<string, readonly [string, string]>> = {
  deposit: ['Nervos DAO deposit', 'Nervos DAO deposits'],
  withdraw_request: ['Nervos DAO withdrawal request', 'Nervos DAO withdrawal requests'],
  withdraw_complete: ['Nervos DAO withdrawal', 'Nervos DAO withdrawals'],
};

/**
 * Whether the activity says whose each Nervos DAO step is (lane P1, third round; review
 * p-logic-chain SHOULD 2). A DAO action names no party; the DAO tag says only that a party's cells
 * took one of the steps. So a step is a party's where one party is tagged, or where every step is
 * of one kind (each tagged party took that kind of step); where several parties are tagged and the
 * steps are of more than one kind, the activity does not say which is whose. (What the route would
 * have to send for it: each DAO action's owner, the lock hash of the cell that took the step.)
 */
export function daoStepsAttributed(activity: {
  protocolActions: GlobalActivity['protocolActions'];
  participants: readonly Pick<ParticipantInfo, 'tags'>[];
}): boolean {
  const kinds = new Set(
    activity.protocolActions.filter((a) => a.protocol === 'dao').map((a) => a.action)
  );
  const tagged = activity.participants.filter((p) => (p.tags & TAG_DAO) !== 0);
  return kinds.size <= 1 || tagged.length <= 1;
}

/**
 * A party's Nervos DAO step: its words, and the step's own amount (the action's capacity, the
 * figure the lede states) where the activity says it is this party's one step of its kind; null
 * where it does not (several parties took steps of that kind, or the activity does not say whose
 * each step is).
 */
export interface DaoStep {
  words: string;
  capacity: bigint | null;
}

/**
 * What a party's Nervos DAO steps are, from the activity's own kind (lane P1 follow-up; review
 * chain-b tx-dao-deposit: What moved drew a deposit to its holder as a payment): each kind of the
 * transaction's DAO actions once, for a party the activity tags DAO (`TAG_DAO`: its cells hold the
 * step), with the step's own amount where it is the party's one step of the kind (lane P1, the
 * lead's look after the third round: a self-deposit's end read "0.00001124 CKB · Nervos DAO
 * deposit", the fee beside the step, as if the deposit were the fee; the step's amount is its
 * own, apart from what flowed); "a Nervos DAO step" where the activity does not say whose each
 * step is (`daoStepsAttributed`), never a step the party did not take; null for any other party,
 * or where the transaction takes no DAO step. A DAO action this page has no words for fails, by
 * the transaction and the action.
 */
export function daoStepsOf(
  activity: {
    txHash: string;
    protocolActions: GlobalActivity['protocolActions'];
    participants: readonly Pick<ParticipantInfo, 'tags'>[];
  },
  party: Pick<ParticipantInfo, 'tags'>
): DaoStep[] | null {
  if ((party.tags & TAG_DAO) === 0) return null;
  const actions = activity.protocolActions.filter((a) => a.protocol === 'dao');
  if (actions.length === 0) return null;
  const kinds = [...new Set(actions.map((a) => a.action))];
  for (const kind of kinds)
    if (!DAO_STEP[kind])
      throw new Error(`${activity.txHash}: a Nervos DAO step "${kind}" has no words`);
  if (!daoStepsAttributed(activity)) return [{ words: 'a Nervos DAO step', capacity: null }];
  const alone = activity.participants.filter((p) => (p.tags & TAG_DAO) !== 0).length === 1;
  return kinds.map((kind) => {
    const [one, several] = DAO_STEP[kind];
    const of = actions.filter((a) => a.action === kind);
    // the party's own steps of this kind are all of them where it is the one party tagged DAO
    if (!alone) return { words: one, capacity: null };
    return of.length === 1
      ? { words: one, capacity: protocolActionMeta(of[0]).capacity }
      : { words: `${of.length.toLocaleString('en-US')} ${several}`, capacity: null };
  });
}

function exact(value: string | number, what: string, hash: string): bigint {
  const s = String(value);
  if (!/^-?\d+$/.test(s)) throw new Error(`${hash}: ${what} ${JSON.stringify(value)}`);
  return BigInt(s);
}

export function flowKey(item: ItemDelta): string {
  return item.kind === 'token'
    ? `token:${item.typeScriptHash}`
    : item.kind === 'object'
      ? `object:${item.objectId}`
      : `identity:${item.standard}:${item.identityId}`;
}

function assetOf(item: ItemDelta): FlowAsset {
  switch (item.kind) {
    case 'token':
      return { kind: 'token', item };
    case 'object':
      return { kind: 'object', item };
    case 'identity':
      return { kind: 'identity', item };
  }
}

interface Entry {
  party: ParticipantInfo;
  amount: bigint;
  index: number;
}

/** Largest amount first; equal amounts keep the API's participant order. */
function partyEnds(entries: Entry[]): FlowEnd[] {
  return [...entries]
    .sort((a, b) => (a.amount === b.amount ? a.index - b.index : a.amount > b.amount ? -1 : 1))
    .map(({ party, amount }) => ({ t: 'party', party, amount }));
}

function sum(ends: readonly FlowEnd[]): bigint {
  return ends.reduce((acc, e) => acc + e.amount, ZERO);
}

/** A side's party ends alone: its terminals (the miner, compensation, minted, burnt) left out. */
function parties(ends: readonly FlowEnd[]): FlowEnd[] {
  return ends.filter((e) => e.t === 'party');
}

/**
 * The flows of a transaction: items first, in order of first appearance (tokens, objects and
 * identities as the participants list them), CKB last — CKB is also the container the items ride
 * in. An item whose receivers got more than its givers gave was minted here (a `minted` giver
 * carries the difference); less, burnt here (a `burnt` receiver). The CKB flow carries the fee
 * as the miner's receipt and DAO compensation as a giver, and must then conserve exactly.
 */
export function txFlows({
  hash,
  activity,
  fee,
  compensation,
}: {
  hash: string;
  activity: Pick<GlobalActivity, 'participants'>;
  fee: string;
  compensation: bigint | null;
}): TxFlow[] {
  const items = new Map<string, { asset: FlowAsset; from: Entry[]; to: Entry[] }>();
  const order: string[] = [];
  const ckbFrom: Entry[] = [];
  const ckbTo: Entry[] = [];
  activity.participants.forEach((party, index) => {
    for (const item of party.itemDeltas) {
      const key = flowKey(item);
      let flow = items.get(key);
      if (!flow) {
        flow = { asset: assetOf(item), from: [], to: [] };
        items.set(key, flow);
        order.push(key);
      }
      const delta = exact(item.delta, `${key} delta`, hash);
      if (delta < ZERO) flow.from.push({ party, amount: -delta, index });
      else if (delta > ZERO) flow.to.push({ party, amount: delta, index });
    }
    const ckb = exact(party.ckbDelta, 'participant ckbDelta', hash);
    if (ckb < ZERO) ckbFrom.push({ party, amount: -ckb, index });
    else if (ckb > ZERO) ckbTo.push({ party, amount: ckb, index });
  });

  const flows: TxFlow[] = order.map((key) => {
    const flow = items.get(key)!;
    const from = partyEnds(flow.from);
    const to = partyEnds(flow.to);
    const diff = sum(to) - sum(from);
    if (diff > ZERO) from.unshift({ t: 'minted', amount: diff });
    else if (diff < ZERO) to.push({ t: 'burnt', amount: -diff });
    return {
      key,
      asset: flow.asset,
      from,
      to,
      total: sum(from),
      sent: sum(parties(from)),
      received: sum(parties(to)),
    };
  });

  const from = partyEnds(ckbFrom);
  const to = partyEnds(ckbTo);
  const feeShannons = exact(fee, 'fee', hash);
  if (feeShannons < ZERO) throw new Error(`${hash}: negative fee ${fee}`);
  if (feeShannons > ZERO) to.push({ t: 'miner', amount: feeShannons });
  if (compensation !== null) {
    if (compensation <= ZERO)
      throw new Error(`${hash}: DAO compensation ${compensation} is not positive`);
    from.unshift({ t: 'compensation', amount: compensation });
  }
  if (from.length > 0 || to.length > 0) {
    const given = sum(from);
    const received = sum(to);
    if (given !== received)
      throw new Error(
        `${hash}: the CKB flow does not conserve: ${given} shannons from the givers, ${received} to the receivers (fee ${feeShannons}, compensation ${compensation ?? 0})`
      );
    flows.push({
      key: 'ckb',
      asset: { kind: 'ckb' },
      from,
      to,
      total: given,
      sent: sum(parties(from)),
      received: sum(parties(to)),
    });
  }
  return flows;
}

/**
 * Whether a transaction moved its fee and nothing else (lane P1, the plan's "a transaction that
 * moves only its fee says so in one line"): it takes no protocol step, and its flows are the CKB
 * flow alone, which ends at the miner and at no party, with no Nervos DAO compensation entering
 * it. A transaction that moved nothing, not even a fee, did not move "only its fee". The one
 * decision the lede, What moved and the balance table follow (lane P1, third round; review
 * p-logic-chain MUST 2: a DAO deposit or withdrawal request from the owner's own address leaves its
 * one party −fee and no item, and read as "only the fee moved").
 */
export function onlyFeeMoved(
  activity: Pick<GlobalActivity, 'protocolActions'>,
  flows: readonly TxFlow[]
): boolean {
  if (activity.protocolActions.length > 0) return false;
  if (flows.length !== 1 || flows[0].asset.kind !== 'ckb') return false;
  const [ckb] = flows;
  return (
    ckb.to.length > 0 &&
    ckb.to.every((e) => e.t === 'miner') &&
    ckb.from.every((e) => e.t === 'party')
  );
}

const SHOWN_WHEN_FOLDED = PARTICIPANT_LIMIT - 1;

/**
 * One side of a flow as drawn: every end up to PARTICIPANT_LIMIT parties (or when expanded);
 * otherwise the five largest parties and one `more` end carrying the rest, their count and their
 * sum. Terminals are never folded: a giving terminal stays first, a receiving one last.
 */
export function foldEnds(ends: readonly FlowEnd[], expanded: boolean): FlowEnd[] {
  const parties = ends.filter((e) => e.t === 'party');
  if (expanded || parties.length <= PARTICIPANT_LIMIT) return [...ends];
  const leading = ends.filter((e) => e.t === 'compensation' || e.t === 'minted');
  const trailing = ends.filter((e) => e.t === 'miner' || e.t === 'burnt');
  const rest = parties.slice(SHOWN_WHEN_FOLDED);
  return [
    ...leading,
    ...parties.slice(0, SHOWN_WHEN_FOLDED),
    { t: 'more', count: rest.length, amount: sum(rest), ends: rest },
    ...trailing,
  ];
}

/** The ribbon's stroke, 2 to 22 px: 2 plus the end's share of its side (exact, to 1/1000). */
export function ribbonWeight(amount: bigint, sideTotal: bigint): number {
  if (sideTotal <= ZERO) throw new Error(`ribbon weight of a side with total ${sideTotal}`);
  if (amount < ZERO || amount > sideTotal)
    throw new Error(`ribbon weight: ${amount} is not within the side's ${sideTotal}`);
  const share = Number((amount * BigInt(1000)) / sideTotal) / 1000;
  return 2 + Math.round(20 * share);
}
