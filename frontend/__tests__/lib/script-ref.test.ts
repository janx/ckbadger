import {
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
