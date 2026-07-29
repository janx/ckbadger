import {
  encodeCellsByScriptCursor,
  getScriptRefBadgeLabel,
  getScriptRefQueryHashType,
  getScriptRefVerboseLabel,
  normalizeScriptRefHashType,
} from '@/lib/script-ref';

describe('script-ref utils', () => {
  it('normalizes known hash types', () => {
    expect(normalizeScriptRefHashType('type')).toBe('type');
    expect(normalizeScriptRefHashType('data')).toBe('data');
    expect(normalizeScriptRefHashType('data1')).toBe('data1');
    expect(normalizeScriptRefHashType('data2')).toBe('data2');
  });

  it('returns null for unknown hash type', () => {
    expect(normalizeScriptRefHashType('weird')).toBeNull();
    expect(normalizeScriptRefHashType(null)).toBeNull();
    expect(normalizeScriptRefHashType(undefined)).toBeNull();
  });

  it('formats badge labels consistently', () => {
    expect(getScriptRefBadgeLabel('type')).toBe('type');
    expect(getScriptRefBadgeLabel('data')).toBe('bytecode(data)');
    expect(getScriptRefBadgeLabel('data1')).toBe('bytecode(data1)');
    expect(getScriptRefBadgeLabel('data2')).toBe('bytecode(data2)');
    expect(getScriptRefBadgeLabel('unknown')).toBe('bytecode(data)');
  });

  it('formats verbose labels consistently', () => {
    expect(getScriptRefVerboseLabel('type')).toBe('type ref');
    expect(getScriptRefVerboseLabel('data')).toBe('bytecode hash ref (data)');
    expect(getScriptRefVerboseLabel('data1')).toBe('bytecode hash ref (data1)');
    expect(getScriptRefVerboseLabel('data2')).toBe('bytecode hash ref (data2)');
    expect(getScriptRefVerboseLabel('unknown')).toBe('bytecode hash ref (data)');
  });

  it('resolves query hash type with fallback', () => {
    expect(getScriptRefQueryHashType('type')).toBe('type');
    expect(getScriptRefQueryHashType('data1')).toBe('data1');
    expect(getScriptRefQueryHashType('unknown')).toBe('type');
    expect(getScriptRefQueryHashType(null, 'data2')).toBe('data2');
  });
});

describe('encodeCellsByScriptCursor', () => {
  const referenceHash = `0x${'9b'.repeat(32)}`;
  const txHash = `0x${'ab'.repeat(32)}`;

  it('encodes the cell-by-code index key with the hash_type byte', () => {
    const cursor = encodeCellsByScriptCursor({
      referenceHash,
      hashType: 'type',
      createdAtBlock: 123,
      txHash,
      outputIndex: 7,
    });

    // code_hash(32) + hash_type(1) + block(8 BE) + tx_hash(32) + index(2 BE)
    expect(cursor).toHaveLength(75 * 2);
    expect(cursor.slice(0, 64)).toBe('9b'.repeat(32));
    expect(cursor.slice(64, 66)).toBe('01');
    expect(cursor.slice(66, 82)).toBe('000000000000007b');
    expect(cursor.slice(82, 146)).toBe('ab'.repeat(32));
    expect(cursor.slice(146)).toBe('0007');
  });

  it('maps each hash type to its protocol byte', () => {
    const byteOf = (hashType: 'data' | 'type' | 'data1' | 'data2') =>
      encodeCellsByScriptCursor({
        referenceHash,
        hashType,
        createdAtBlock: 1,
        txHash,
        outputIndex: 0,
      }).slice(64, 66);

    expect(byteOf('data')).toBe('00');
    expect(byteOf('type')).toBe('01');
    expect(byteOf('data1')).toBe('02');
    expect(byteOf('data2')).toBe('04');
  });

  it('prefixes the enumeration phase for script_kind=both', () => {
    const key = encodeCellsByScriptCursor({
      referenceHash,
      hashType: 'type',
      createdAtBlock: 5,
      txHash,
      outputIndex: 0,
    });

    expect(
      encodeCellsByScriptCursor({
        referenceHash,
        hashType: 'type',
        createdAtBlock: 5,
        txHash,
        outputIndex: 0,
        phase: 'type',
      })
    ).toBe(`type:${key}`);
  });

  it('rejects malformed hex input instead of emitting a corrupt cursor', () => {
    expect(() =>
      encodeCellsByScriptCursor({
        referenceHash: '0x9b9',
        hashType: 'type',
        createdAtBlock: 1,
        txHash,
        outputIndex: 0,
      })
    ).toThrow(/Invalid hex length/);
  });
});
