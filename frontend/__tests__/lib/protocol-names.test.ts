import { describe, expect, it } from 'vitest';
import { protocolDisplayName } from '@/lib/protocol-names';

describe('protocolDisplayName', () => {
  it('names every protocol the indexer emits', () => {
    expect(protocolDisplayName('fiber')).toBe('Fiber');
    expect(protocolDisplayName('rgbpp')).toBe('RGB++');
    expect(protocolDisplayName('stablepp')).toBe('Stable++');
    expect(protocolDisplayName('utxoswap')).toBe('UTXOSwap');
    expect(protocolDisplayName('dotcell')).toBe('.cell');
    expect(protocolDisplayName('dao')).toBe('Nervos DAO');
  });

  it('shows an id it does not know as the id itself, the fact the API gave', () => {
    expect(protocolDisplayName('newproto')).toBe('newproto');
  });
});
