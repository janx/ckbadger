/**
 * The transaction page's read model (F2-13, Main.dc.html v5): which cells, script groups,
 * witnesses and deps a hover links, the balance table's Net row, the capacity equation and the
 * plain movement summary. Every input is an API field; nothing here re-derives what a script or
 * protocol means — it links and adds what the API already states, and fails loudly when the
 * API's own numbers disagree.
 */
import type { ItemDelta, ParticipantInfo } from '@/lib/api';
import { SHANNONS_PER_CKB } from '@/lib/ckb-decimal';

export interface TxLinkGroup {
  id: string;
  cellIds: readonly string[];
  witnessIndex: number | null;
  scriptHash: string;
}
export interface TxLinkDep {
  index: number;
  /** Script hashes whose code this dep supplies (API `/cell-deps`). */
  scriptHashes: readonly string[];
}
export type TxHover =
  | { t: 'cell'; id: string }
  | { t: 'cells'; ids: readonly string[] }
  | { t: 'group'; id: string }
  | { t: 'witness'; index: number }
  | { t: 'dep'; index: number };
export interface TxLinkSets {
  cells: ReadonlySet<string>;
  groups: ReadonlySet<string>;
  witnesses: ReadonlySet<number>;
  deps: ReadonlySet<number>;
}

/**
 * What a hover lights (board v5): a cell lights itself, its groups, their witnesses and deps; a
 * group, witness or dep lights that group (or the groups it serves), their cells, witness and dep.
 * Groups are linked by identity, never because they share a cell with the hovered one (TX-14).
 */
export function txLinks(
  hover: TxHover,
  groups: readonly TxLinkGroup[],
  deps: readonly TxLinkDep[],
  hash: string
): TxLinkSets {
  const cells = new Set<string>();
  const lit = new Set<string>();
  const witnesses = new Set<number>();
  const depSet = new Set<number>();
  const depsOf = (g: TxLinkGroup) =>
    deps.filter((d) => d.scriptHashes.includes(g.scriptHash)).map((d) => d.index);
  const evidence = (g: TxLinkGroup) => {
    lit.add(g.id);
    if (g.witnessIndex !== null) witnesses.add(g.witnessIndex);
    depsOf(g).forEach((i) => depSet.add(i));
  };
  const whole = (g: TxLinkGroup) => {
    evidence(g);
    g.cellIds.forEach((c) => cells.add(c));
  };
  switch (hover.t) {
    case 'cell':
    case 'cells': {
      const ids = hover.t === 'cell' ? [hover.id] : hover.ids;
      ids.forEach((c) => cells.add(c));
      groups.filter((g) => g.cellIds.some((c) => ids.includes(c))).forEach(evidence);
      break;
    }
    case 'group': {
      const g = groups.find((x) => x.id === hover.id);
      if (!g) throw new Error(`${hash}: no script group ${hover.id}`);
      whole(g);
      break;
    }
    case 'witness':
      witnesses.add(hover.index);
      groups.filter((g) => g.witnessIndex === hover.index).forEach(whole);
      break;
    case 'dep': {
      const d = deps.find((x) => x.index === hover.index);
      if (!d) throw new Error(`${hash}: no cell dep ${hover.index}`);
      depSet.add(d.index);
      groups.filter((g) => d.scriptHashes.includes(g.scriptHash)).forEach(whole);
      break;
    }
  }
  return { cells, groups: lit, witnesses, deps: depSet };
}

function exact(value: string, what: string, hash: string): bigint {
  if (!/^-?\d+$/.test(value)) throw new Error(`${hash}: ${what} ${JSON.stringify(value)}`);
  return BigInt(value);
}

/**
 * The Net row of Balance changes: the participants' changes added up, and what the difference
 * is — the fee paid to the miner, or the DAO compensation less the fee. A sum that is neither is
 * a broken invariant upstream. Genesis transactions issue capacity and pay no fee.
 */
export function balanceNet({
  hash,
  participants,
  fee,
  compensation,
  genesis,
}: {
  hash: string;
  participants: readonly Pick<ParticipantInfo, 'ckbDelta' | 'usedDelta'>[];
  fee: string;
  compensation: bigint | null;
  genesis: boolean;
}): { ckb: bigint; occupied: bigint; note: 'fee' | 'compensation' | 'genesis' } {
  let ckb = BigInt(0);
  let occupied = BigInt(0);
  for (const p of participants) {
    ckb += exact(p.ckbDelta, 'participant ckbDelta', hash);
    occupied += exact(p.usedDelta, 'participant usedDelta', hash);
  }
  if (genesis) return { ckb, occupied, note: 'genesis' };
  const expected = (compensation ?? BigInt(0)) - exact(fee, 'fee', hash);
  if (ckb !== expected)
    throw new Error(`${hash}: participant CKB changes sum to ${ckb}, expected ${expected}`);
  return { ckb, occupied, note: compensation === null ? 'fee' : 'compensation' };
}

/**
 * The exact capacity equation: inputs (+ DAO compensation) = outputs + fee. Conservation is a
 * consensus rule, so a mismatch is an API bug, never something to round over.
 */
export function capacityEquation({
  hash,
  inputs,
  outputs,
  fee,
  compensation,
}: {
  hash: string;
  inputs: string;
  outputs: string;
  fee: string;
  compensation: bigint | null;
}): { inputs: bigint; outputs: bigint; fee: bigint; compensation: bigint | null } {
  const result = {
    inputs: exact(inputs, 'inputs capacity', hash),
    outputs: exact(outputs, 'outputs capacity', hash),
    fee: exact(fee, 'fee', hash),
    compensation,
  };
  if (result.inputs + (compensation ?? BigInt(0)) !== result.outputs + result.fee)
    throw new Error(
      `${hash}: capacity is not conserved: ${result.inputs} in + ${compensation ?? 0} compensation ≠ ${result.outputs} out + ${result.fee} fee`
    );
  return result;
}

export interface TxMovement {
  /** The item that moved; `null` for plain CKB. */
  item: ItemDelta | null;
  from: ParticipantInfo[];
  to: ParticipantInfo[];
}

function itemKey(item: ItemDelta): string {
  return item.kind === 'token'
    ? `token:${item.typeScriptHash}`
    : item.kind === 'object'
      ? `object:${item.objectId}`
      : `identity:${item.standard}:${item.identityId}`;
}

function signOf(item: ItemDelta): bigint {
  return BigInt(String(item.delta));
}

/**
 * What moved between whom, read off the participants' signed changes (TX-11): each item from the
 * parties whose delta is negative to those whose delta is positive; with no item changes and no
 * protocol action, the CKB itself. A transaction a protocol action describes says it in the
 * action's own words instead.
 */
export function txMovements(activity: {
  participants: readonly ParticipantInfo[];
  protocolActions: readonly unknown[];
}): TxMovement[] {
  const moves = new Map<string, TxMovement>();
  for (const p of activity.participants)
    for (const item of p.itemDeltas) {
      const key = itemKey(item);
      const move = moves.get(key) ?? { item, from: [], to: [] };
      const sign = signOf(item);
      if (sign < BigInt(0)) move.from.push(p);
      else if (sign > BigInt(0)) move.to.push(p);
      moves.set(key, move);
    }
  if (moves.size > 0) return [...moves.values()];
  if (activity.protocolActions.length > 0) return [];
  const from = activity.participants.filter((p) => BigInt(p.ckbDelta) < BigInt(0));
  const to = activity.participants.filter((p) => BigInt(p.ckbDelta) > BigInt(0));
  return to.length > 0 ? [{ item: null, from, to }] : [];
}

/** A column-head capacity: whole CKB, with `…` when a fraction is dropped (`42,081.5…`). */
export function shortCkb(shannons: bigint): string {
  if (shannons < BigInt(0)) throw new Error(`shortCkb: negative capacity ${shannons}`);
  const whole = (shannons / SHANNONS_PER_CKB).toLocaleString('en-US');
  const frac = shannons % SHANNONS_PER_CKB;
  if (frac === BigInt(0)) return whole;
  return `${whole}.${frac.toString().padStart(8, '0')[0]}…`;
}
