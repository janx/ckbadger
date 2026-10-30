/** Strict chain-byte decoding for visual representations, never a guessed payload. */
export function hexBytes(value: string, context: string): string[] {
  const hex = value.startsWith('0x') ? value.slice(2) : value;
  if (hex.length % 2 !== 0 || !/^[0-9a-f]*$/i.test(hex))
    throw new Error(`${context}: expected byte-aligned hexadecimal data`);
  return Array.from({ length: hex.length / 2 }, (_, i) =>
    hex.slice(i * 2, i * 2 + 2).toLowerCase()
  );
}
export interface ByteRange {
  id: string;
  label: string;
  offset: number;
  length: number;
  description: string;
}
export function validateByteRanges(
  ranges: readonly ByteRange[],
  byteLength: number,
  context: string
): void {
  const ids = new Set<string>();
  for (const range of ranges) {
    if (ids.has(range.id)) throw new Error(`${context}: duplicate field ${range.id}`);
    ids.add(range.id);
    if (
      !Number.isSafeInteger(range.offset) ||
      !Number.isSafeInteger(range.length) ||
      range.offset < 0 ||
      range.length <= 0 ||
      range.offset + range.length > byteLength
    )
      throw new Error(
        `${context}: field ${range.id} range ${range.offset}+${range.length} exceeds ${byteLength} bytes`
      );
  }
}

/** An exact decimal u64 (e.g. a `since`) as its 8-byte value and its little-endian bytes (FF-C1). */
export function u64Hex(decimal: string, context: string): { value: string; littleEndian: string } {
  if (!/^(0|[1-9]\d*)$/.test(decimal) || BigInt(decimal) >= BigInt(1) << BigInt(64))
    throw new Error(`${context}: ${decimal} is not a u64`);
  const hex = BigInt(decimal).toString(16).padStart(16, '0');
  const le = (hex.match(/../g) as string[]).reverse().join('');
  return { value: `0x${hex}`, littleEndian: `0x${le}` };
}
