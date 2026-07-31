import { parseRawSourcePath } from '@/lib/ai/raw-route';

describe('parseRawSourcePath', () => {
  it('parses supported raw routes', () => {
    expect(parseRawSourcePath('/blocks/123')).toEqual({
      kind: 'block_detail',
      pathname: '/blocks/123',
      id: '123',
    });

    expect(parseRawSourcePath('/tx/0xabc')).toEqual({
      kind: 'tx_detail',
      pathname: '/tx/0xabc',
      hash: '0xabc',
    });

    expect(parseRawSourcePath('/cell/0xabc-1')).toEqual({
      kind: 'cell_detail',
      pathname: '/cell/0xabc-1',
      outpoint: '0xabc-1',
    });

    expect(parseRawSourcePath('/identities/dotbit/0xdotbit')).toEqual({
      kind: 'dotbit_item_detail',
      pathname: '/identities/dotbit/0xdotbit',
      identityId: '0xdotbit',
    });

    expect(parseRawSourcePath('/identities/did/0xdid')).toEqual({
      kind: 'did_ckb_item_detail',
      pathname: '/identities/did/0xdid',
      identityId: '0xdid',
    });

    expect(parseRawSourcePath('/identities/bit-cell/0xbitcell')).toEqual({
      kind: 'bit_cell_item_detail',
      pathname: '/identities/bit-cell/0xbitcell',
      identityId: '0xbitcell',
    });

    expect(parseRawSourcePath('/objects/mnft/0xmnft')).toEqual({
      kind: 'mnft_item_detail',
      pathname: '/objects/mnft/0xmnft',
      objectId: '0xmnft',
    });
  });

  it('returns unknown for unsupported raw route', () => {
    expect(parseRawSourcePath('/blocks')).toEqual({
      kind: 'unknown',
      pathname: '/blocks',
    });
    expect(parseRawSourcePath('/charts/hash-rate')).toEqual({
      kind: 'unknown',
      pathname: '/charts/hash-rate',
    });
  });
});
