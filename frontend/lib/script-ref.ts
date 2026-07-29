export type ScriptRefHashType = 'type' | 'data' | 'data1' | 'data2';

export function normalizeScriptRefHashType(
  value: string | null | undefined
): ScriptRefHashType | null {
  if (value === 'type' || value === 'data' || value === 'data1' || value === 'data2') {
    return value;
  }
  return null;
}

export function getScriptRefQueryHashType(
  value: string | null | undefined,
  fallback: ScriptRefHashType = 'type'
): ScriptRefHashType {
  return normalizeScriptRefHashType(value) ?? fallback;
}

export function getScriptRefBadgeLabel(value: string | null | undefined): string {
  const normalized = normalizeScriptRefHashType(value);
  if (normalized === 'type') {
    return 'type';
  }
  return `bytecode(${normalized ?? 'data'})`;
}

export function getScriptRefVerboseLabel(value: string | null | undefined): string {
  const normalized = normalizeScriptRefHashType(value);
  if (normalized === 'type') {
    return 'type ref';
  }
  return `bytecode hash ref (${normalized ?? 'data'})`;
}

const SCRIPT_REF_HASH_TYPE_BYTE: Record<ScriptRefHashType, number> = {
  data: 0,
  type: 1,
  data1: 2,
  data2: 4,
};

function hexToBytes(hex: string): number[] {
  const normalized = hex.startsWith('0x') ? hex.slice(2) : hex;
  if (normalized.length % 2 !== 0) {
    throw new Error(`Invalid hex length for cursor encoding: ${hex}`);
  }

  const bytes: number[] = [];
  for (let index = 0; index < normalized.length; index += 2) {
    const pair = normalized.slice(index, index + 2);
    const value = Number.parseInt(pair, 16);
    if (Number.isNaN(value)) {
      throw new Error(`Invalid hex value for cursor encoding: ${hex}`);
    }
    bytes.push(value);
  }
  return bytes;
}

/**
 * Build a `/cells/by-script` cursor from the last row a page consumed.
 *
 * The key mirrors the cell-by-code index key the API paginates on:
 * `code_hash(32) + hash_type(1) + block(8 BE) + tx_hash(32) + output_index(2 BE)`.
 * `script_kind=both` additionally needs the enumeration phase, so the key is
 * prefixed with the row's `matchedScriptKind` — pass `phase` only in that mode.
 */
export function encodeCellsByScriptCursor(params: {
  referenceHash: string;
  hashType: ScriptRefHashType;
  createdAtBlock: number;
  txHash: string;
  outputIndex: number;
  phase?: 'lock' | 'type';
}): string {
  const bytes = [
    ...hexToBytes(params.referenceHash),
    SCRIPT_REF_HASH_TYPE_BYTE[params.hashType],
    ...Array.from(
      new Uint8Array(new BigInt64Array([BigInt(params.createdAtBlock)]).buffer).reverse()
    ),
    ...hexToBytes(params.txHash),
    ...Array.from(new Uint8Array(new Int16Array([params.outputIndex]).buffer).reverse()),
  ];
  const key = bytes.map((value) => value.toString(16).padStart(2, '0')).join('');
  return params.phase ? `${params.phase}:${key}` : key;
}
