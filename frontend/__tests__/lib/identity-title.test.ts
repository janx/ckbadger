import { describe, expect, it } from 'vitest';
import type { ItemProtocol } from '@/lib/api';
import { identityTitleParts } from '@/lib/identity-title';

// Only the fields the naming reads; the rest of a protocol version is not consulted.
function version<T>(decoded: T) {
  return { cell: {}, dataHex: '0x', fields: [], decoded } as never;
}
const did = (document: unknown) =>
  ({
    kind: 'did_ckb',
    itemId: '0x02',
    versions: [version({ did: 'did:ckb:old' }), version({ did: 'did:ckb:almulti', document })],
  }) as unknown as Extract<ItemProtocol, { kind: 'did_ckb' }>;

describe('identityTitleParts (the identity page and the cell inventory line, one path)', () => {
  it('names a did:ckb by the first handle of its latest document, the domain apart', () => {
    expect(
      identityTitleParts('did', 'fallback', did({ alsoKnownAs: ['at://3333.web5.bbs.fans'] }))
    ).toEqual({ name: '3333', suffix: '.web5.bbs.fans', full: '3333.web5.bbs.fans' });
  });

  it('names a did:ckb without a handle by its DID, and by the caller title before the read', () => {
    expect(identityTitleParts('did', 'fallback', did({}))).toEqual({
      name: 'did:ckb:almulti',
      suffix: '',
      full: 'did:ckb:almulti',
    });
    expect(identityTitleParts('did', 'did:ckb:x', undefined).full).toBe('did:ckb:x');
  });

  it('names .bit and .bit Cell by the latest decoded account, with the .bit suffix apart', () => {
    const bitCell = {
      kind: 'bit_cell',
      itemId: '0x00',
      versions: [version({ account: 'old.bit' }), version({ account: '888899.bit' })],
    } as unknown as Extract<ItemProtocol, { kind: 'bit_cell' }>;
    expect(identityTitleParts('bit-cell', '', bitCell)).toEqual({
      name: '888899',
      suffix: '.bit',
      full: '888899.bit',
    });
  });

  it('names .cell by the title it is given', () => {
    expect(identityTitleParts('dotcell', 'verification.cell', undefined)).toEqual({
      name: 'verification',
      suffix: '.cell',
      full: 'verification.cell',
    });
  });
});
