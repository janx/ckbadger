/**
 * The one reader of protocol-action metadata. The API serves free-form JSON (protocol-action
 * metadata, decoded lock args) with every integer as an exact decimal string, because a JSON
 * number above 2^53 — 90,071,992 CKB in shannons — is rounded by the browser before any code sees
 * it. So integers are read with BigInt from strings, and a JSON number is a contract violation.
 */
import type { ActivityProtocolAction } from '@/lib/api';

export interface ProtocolActionMeta {
  /** Shannons. */
  capacity: bigint | null;
  /** Shannons. */
  compensation: bigint | null;
  depositBlock: bigint | null;
  channelOutpoint: string | null;
  btcTxid: string | null;
}

function where(pa: ActivityProtocolAction, key: string): string {
  return `${pa.protocol}:${pa.action} metadata.${key}`;
}

function integerOf(pa: ActivityProtocolAction, key: string): bigint | null {
  const v: unknown = pa.metadata?.[key];
  if (v === undefined || v === null) return null;
  if (typeof v === 'number') {
    throw new Error(`${where(pa, key)} is the JSON number ${v}, not an exact integer string`);
  }
  if (typeof v !== 'string' || !/^-?\d+$/.test(v)) {
    throw new Error(`${where(pa, key)} ${JSON.stringify(v)} is not an integer`);
  }
  return BigInt(v);
}

function stringOf(pa: ActivityProtocolAction, key: string): string | null {
  const v: unknown = pa.metadata?.[key];
  if (v === undefined || v === null) return null;
  if (typeof v !== 'string')
    throw new Error(`${where(pa, key)} ${JSON.stringify(v)} is not a string`);
  return v;
}

/** The facts a protocol action's metadata carries, read once for every renderer. */
export function protocolActionMeta(pa: ActivityProtocolAction): ProtocolActionMeta {
  return {
    capacity: integerOf(pa, 'capacity'),
    compensation: integerOf(pa, 'compensation'),
    depositBlock: integerOf(pa, 'depositBlock'),
    channelOutpoint: stringOf(pa, 'channelOutpoint'),
    btcTxid: stringOf(pa, 'btcTxid'),
  };
}
