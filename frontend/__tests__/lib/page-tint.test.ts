import { describe, expect, it } from 'vitest';
import { PAGE_TINTS, tintOfPath } from '@/lib/page-tint';

// Material pass v2 (move 1): the ambient pools take the page's class hue, named by the route.
describe('tintOfPath', () => {
  it('names the five tints', () => {
    expect(PAGE_TINTS).toEqual(['jade', 'gold', 'azure', 'violet', 'rose']);
  });

  it('is gold on the DAO pages', () => {
    expect(tintOfPath('/dao')).toBe('gold');
    expect(tintOfPath('/dao/depositors')).toBe('gold');
  });

  it('is azure on token pages, violet on object, cluster and class pages, rose on identities', () => {
    expect(tintOfPath('/tokens/0xabc')).toBe('azure');
    expect(tintOfPath('/inventory/tokens')).toBe('azure');
    expect(tintOfPath('/objects/0xabc')).toBe('violet');
    expect(tintOfPath('/objects/mnft/0xabc')).toBe('violet');
    expect(tintOfPath('/clusters/0xabc')).toBe('violet');
    expect(tintOfPath('/classes/0xabc')).toBe('violet');
    expect(tintOfPath('/inventory/objects')).toBe('violet');
    expect(tintOfPath('/identities/dotbit/alice.bit')).toBe('rose');
    expect(tintOfPath('/identities/0xabc')).toBe('rose');
    expect(tintOfPath('/inventory/identities')).toBe('rose');
  });

  it('is jade everywhere else: the home page, blocks, a transaction, an address, the charts', () => {
    for (const path of [
      '/',
      '/blocks',
      '/blocks/1',
      '/tx/0xabc',
      '/address/ckb1q',
      '/charts/hash-rate',
      '/network',
      '/scripts',
      '/fiber/channels',
    ])
      expect(tintOfPath(path)).toBe('jade');
  });

  it('refuses a relative path: the tint is read from the route, which is absolute', () => {
    expect(() => tintOfPath('dao')).toThrow('page tint: path "dao" is not absolute');
  });
});
