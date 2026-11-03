import type { Cell } from '@/lib/api';
import {
  getClusterDetailHref,
  getIdentityItemDetailHref,
  getMnftClassDetailHref,
  getObjectDetailHref,
  getTokenDetailHref,
} from '@/lib/detail-routes';
import { formatTokenBalanceWithRawMarker } from '@/lib/format-asset';
import type { IdentityItemKind } from '@/lib/identity-title';

/**
 * Lane U (U6): the registry protocol slugs (`cell.protocolScript.type`, the API's one protocol
 * identity — never a code hash compared here) whose cell is an inventory item the cell page names
 * in its hero. Any other slug, or none, names no item: the page shows no line rather than a guess.
 * Left out on purpose: `m-nft-issuer` (no page of its own), `ckbfs`, `dotcell-price`, the Stable++
 * cells and every lock-side protocol (they are not items someone holds).
 */
export const CELL_INVENTORY_KINDS = [
  'spore',
  'spore-cluster',
  'm-nft',
  'm-nft-class',
  'xudt',
  'simple-udt',
  'bit-account',
  'bit-cell',
  'did-ckb',
  'dotcell-account',
  'nervos-dao',
] as const;
export type CellInventoryKind = (typeof CELL_INVENTORY_KINDS)[number];

/** What a cell is, as an inventory item, and the page that item has. */
export type CellInventoryItem =
  /** A Spore object; `clusterId` from the cell's own data (null = no cluster). */
  | { kind: 'spore'; id: string; clusterId: string | null; href: string }
  | { kind: 'spore-cluster'; id: string; href: string }
  | { kind: 'm-nft'; id: string; href: string }
  | { kind: 'm-nft-class'; id: string; href: string }
  /** A token cell: `amount` exact (trailing fraction zeros dropped), `unit` its symbol or name. */
  | { kind: 'token'; typeHash: string; amount: string; unit: string; href: string }
  /**
   * A cell of a token type whose data is too short for an amount (owner-mode / non-standard cells
   * the type script admits; the indexer counts no holding for them): `reason` is the API's.
   */
  | { kind: 'token-without-amount'; typeHash: string; unit: string; reason: string; href: string }
  /**
   * An identity: `name` from the cell's decoded data (`.bit`, `.cell`); null where the cell
   * endpoint decodes none (`.bit Cell`, did:ckb) — the line then reads it as their pages do.
   */
  | { kind: 'identity'; standard: IdentityItemKind; id: string; name: string | null; href: string }
  /** A Nervos DAO cell: a deposit or a withdrawal request of its whole capacity. */
  | { kind: 'dao'; stage: 'deposit' | 'withdraw-request'; capacity: bigint; href: '/dao' };

const BYTES32 = /^0x[0-9a-f]{64}$/;
const HEX = /^0x(?:[0-9a-f]{2})+$/;

function isKind(slug: string): slug is CellInventoryKind {
  return (CELL_INVENTORY_KINDS as readonly string[]).includes(slug);
}

function shortHash(hash: string): string {
  return `${hash.slice(0, 10)}…${hash.slice(-6)}`;
}

/**
 * The inventory item this cell is, from its `protocolScript.type` slug; null when the slug is none
 * or one this page does not name. A handled slug whose identifying data is missing or malformed
 * throws with the out point: the line must never name a wrong item.
 */
export function cellInventoryItem(cell: Cell): CellInventoryItem | null {
  const at = `Cell ${cell.txHash}:${cell.outputIndex}`;
  if (!cell.protocolScript) throw new Error(`${at}: the cell detail carries no protocolScript`);
  const slug = cell.protocolScript.type;
  if (slug === null || !isKind(slug)) return null;
  const type = cell.type;
  if (!type)
    throw new Error(`${at}: protocolScript.type is ${slug} but the cell has no type script`);
  const deterministic = cell.dataAnalysis?.deterministic;
  const segment = (label: string): string | undefined =>
    deterministic?.segments.find((s) => s.label === label)?.humanValue;
  const expectKind = (expected: readonly string[]): string => {
    const kind = deterministic?.kind;
    if (kind === undefined || !expected.includes(kind))
      throw new Error(
        `${at}: ${slug} cell without its decoded data (got ${kind === undefined ? 'none' : kind})`
      );
    return kind;
  };
  const args = (re: RegExp, what: string): string => {
    if (!re.test(type.args))
      throw new Error(`${at}: ${slug} type args are not ${what}: ${type.args}`);
    return type.args;
  };
  switch (slug) {
    case 'spore': {
      const id = args(BYTES32, 'a 32-byte id');
      expectKind(['spore_cell']);
      const clusterId = segment('cluster_id') ?? null;
      if (clusterId !== null && !BYTES32.test(clusterId))
        throw new Error(`${at}: spore cluster_id is not a 32-byte id: ${clusterId}`);
      return { kind: 'spore', id, clusterId, href: getObjectDetailHref(id) };
    }
    case 'spore-cluster': {
      const id = args(BYTES32, 'a 32-byte id');
      return { kind: 'spore-cluster', id, href: getClusterDetailHref(id) };
    }
    case 'm-nft': {
      // issuer id 20 B ‖ class index 4 B ‖ token index 4 B
      const id = args(/^0x[0-9a-f]{56}$/, 'a 28-byte m-NFT token id');
      return { kind: 'm-nft', id, href: `/objects/mnft/${id}` };
    }
    case 'm-nft-class': {
      const id = args(/^0x[0-9a-f]{48}$/, 'a 24-byte m-NFT class id');
      return { kind: 'm-nft-class', id, href: getMnftClassDetailHref(id) };
    }
    case 'xudt':
    case 'simple-udt': {
      const typeHash = cell.typeScriptHash;
      if (!typeHash || !BYTES32.test(typeHash))
        throw new Error(`${at}: ${slug} cell without a type script hash`);
      const token = cell.dataAnalysis?.token;
      if (token && token.rawAmount === null) {
        if (!token.noAmountReason)
          throw new Error(`${at}: ${slug} cell with no token amount and no reason for none`);
        return {
          kind: 'token-without-amount',
          typeHash,
          unit: token.symbol || token.name || `token ${shortHash(typeHash)}`,
          reason: token.noAmountReason,
          href: getTokenDetailHref(typeHash),
        };
      }
      if (!token || token.rawAmount === null || !/^(0|[1-9][0-9]*)$/.test(token.rawAmount))
        throw new Error(`${at}: ${slug} cell without its decoded token amount`);
      return {
        kind: 'token',
        typeHash,
        amount: formatTokenBalanceWithRawMarker(token.rawAmount, token.decimals),
        unit: token.symbol || token.name || `units of token ${shortHash(typeHash)}`,
        href: getTokenDetailHref(typeHash),
      };
    }
    case 'bit-account': {
      expectKind(['dotbit_account']);
      const id = segment('account_id'),
        name = segment('account');
      if (!id || !/^0x[0-9a-f]{40}$/.test(id))
        throw new Error(`${at}: .bit account cell without a 20-byte account_id (got ${id})`);
      if (!name?.endsWith('.bit'))
        throw new Error(`${at}: .bit account cell without its account name (got ${name})`);
      return {
        kind: 'identity',
        standard: 'dotbit',
        id,
        name,
        href: getIdentityItemDetailHref('dotbit', id),
      };
    }
    case 'bit-cell':
    case 'did-ckb': {
      const id = args(HEX, 'an identity id');
      const standard = slug === 'bit-cell' ? 'bit-cell' : 'did';
      return {
        kind: 'identity',
        standard,
        id,
        name: null,
        href: getIdentityItemDetailHref(slug === 'bit-cell' ? 'bit-cell' : 'did_ckb', id),
      };
    }
    case 'dotcell-account': {
      // The ring root shares the name cells' type script; it is the protocol's, not a name.
      if (expectKind(['dotcell_name', 'dotcell_ring_root']) === 'dotcell_ring_root') return null;
      // `verification.cell · id 0x0b34…` — the name and its 20-byte identity id.
      const match = /^(\S+\.cell) · id (0x[0-9a-f]{40})$/.exec(segment('label') ?? '');
      if (!match)
        throw new Error(
          `${at}: .cell name cell without its decoded label (got ${segment('label')})`
        );
      return {
        kind: 'identity',
        standard: 'dotcell',
        id: match[2],
        name: match[1],
        href: getIdentityItemDetailHref('dotcell', match[1]),
      };
    }
    case 'nervos-dao': {
      const kind = deterministic?.kind;
      if (kind !== 'dao_deposit_cell' && kind !== 'dao_withdraw_request_cell')
        throw new Error(
          `${at}: nervos-dao cell is neither a deposit nor a withdraw request (got ${kind ?? 'none'})`
        );
      if (!/^(0|[1-9][0-9]*)$/.test(cell.capacity))
        throw new Error(`${at}: DAO cell capacity is not a shannon integer: ${cell.capacity}`);
      return {
        kind: 'dao',
        stage: kind === 'dao_deposit_cell' ? 'deposit' : 'withdraw-request',
        capacity: BigInt(cell.capacity),
        href: '/dao',
      };
    }
  }
}
