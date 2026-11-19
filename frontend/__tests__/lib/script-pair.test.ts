import { distinctScriptPairs, scriptLookupKey } from '@/lib/script-pair';

// Lane rd/api-lock-names: CKB names a script by its pair (code hash, hash type); the script
// lookup is asked and answers by the pair.
describe('script pairs', () => {
  it('keys a pair by its lowercase code hash and its hash type', () => {
    expect(scriptLookupKey('0xABcd', 'type')).toBe('0xabcd:type');
    expect(scriptLookupKey('0xabcd', 'data')).not.toBe(scriptLookupKey('0xabcd', 'type'));
  });

  it('keeps each pair once, the same code hash by two hash types as two scripts', () => {
    const lock = { codeHash: '0xABC', hashType: 'type', args: '0x01' };
    expect(
      distinctScriptPairs([
        lock,
        { codeHash: '0xabc', hashType: 'type' },
        { codeHash: '0xabc', hashType: 'data' },
        null,
        undefined,
      ])
    ).toEqual([
      { codeHash: '0xABC', hashType: 'type' },
      { codeHash: '0xabc', hashType: 'data' },
    ]);
  });
});
