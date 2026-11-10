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
import type { GlobalActivity, ItemDelta, ParticipantInfo } from '@/lib/api';
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
}

const ZERO = BigInt(0);

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
    return { key, asset: flow.asset, from, to, total: sum(from) };
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
    flows.push({ key: 'ckb', asset: { kind: 'ckb' }, from, to, total: given });
  }
  return flows;
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
