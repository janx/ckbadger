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

    expect(parseRawSourcePath('/nfts/dotbit/0xdotbit')).toEqual({
      kind: 'dotbit_item_detail',
      pathname: '/nfts/dotbit/0xdotbit',
      nftId: '0xdotbit',
    });

    expect(parseRawSourcePath('/nfts/did/0xdid')).toEqual({
      kind: 'did_ckb_item_detail',
      pathname: '/nfts/did/0xdid',
      nftId: '0xdid',
    });

    expect(parseRawSourcePath('/nfts/mnft/0xmnft')).toEqual({
      kind: 'mnft_item_detail',
      pathname: '/nfts/mnft/0xmnft',
      nftId: '0xmnft',
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
