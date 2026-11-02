import { describe, expect, it } from 'vitest';
import { PROTOCOL_FIELD_MEANINGS, withFieldMeanings } from '@/lib/protocol-field-meanings';

// The field names the API's `/protocol` routes emit (crates/api/src/routes/asset_protocol.rs).
const API_FIELDS = {
  spore_cluster: ['name', 'description', 'mutantId'],
  spore: ['contentType', 'content', 'clusterId'],
  'm-nft': ['version', 'characteristic', 'configure', 'state'],
  dotbit: ['accountDataHash', 'accountId', 'nextAccountId', 'expiredAt', 'account'],
  bit_cell: ['prefix', 'version', 'witnessHash', 'expiredAt', 'account'],
  did_ckb: ['union', 'document'],
  dotcell: ['layoutVersion', 'recordsHash', 'nextId', 'expiredAt', 'owner', 'manager', 'label'],
} as const;

describe('protocol field meanings (lane E, C5)', () => {
  it('has one sentence for every field each protocol route emits', () => {
    for (const [kind, names] of Object.entries(API_FIELDS))
      for (const name of names) {
        const meaning = PROTOCOL_FIELD_MEANINGS[kind as keyof typeof API_FIELDS][name];
        expect(meaning, `${kind}.${name}`).toMatch(/^[A-Z].*\.$/);
      }
  });

  it('attaches the meaning to each field and leaves an unknown field to its bytes', () => {
    expect(
      withFieldMeanings('dotcell', [
        { name: 'owner', start: 58, end: 78, valueHex: '0x' },
        { name: 'futureField', start: 98, end: 99, valueHex: '0x' },
      ])
    ).toEqual([
      {
        name: 'owner',
        start: 58,
        end: 78,
        valueHex: '0x',
        meaning: 'The first 20 bytes of the owner’s lock hash.',
      },
      { name: 'futureField', start: 98, end: 99, valueHex: '0x' },
    ]);
  });
});
