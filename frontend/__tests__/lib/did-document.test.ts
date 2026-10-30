import { readDidDocument, splitHandle } from '@/lib/did-document';

const jler = {
  alsoKnownAs: ['at://jler.web5.bbs.fans'],
  services: {
    atproto_pds: { endpoint: 'https://web5.bbs.fans', type: 'AtprotoPersonalDataServer' },
  },
  verificationMethods: { atproto: 'did:key:zQ3shaztKtSbMi6Nx5syveg5ZnbdAJT2Fb56hmyB4UDYKWzeT' },
};

describe('readDidDocument (ID-D1 / ID-D2)', () => {
  it('reads the handle, the personal data server and the signing keys as the document states them', () => {
    expect(readDidDocument(jler)).toEqual({
      handles: ['jler.web5.bbs.fans'],
      services: [
        {
          name: 'atproto_pds',
          endpoint: 'https://web5.bbs.fans',
          type: 'AtprotoPersonalDataServer',
        },
      ],
      signingKeys: [
        { name: 'atproto', key: 'did:key:zQ3shaztKtSbMi6Nx5syveg5ZnbdAJT2Fb56hmyB4UDYKWzeT' },
      ],
      unreadable: [],
    });
  });

  it('reports a field it cannot read instead of guessing at it', () => {
    const view = readDidDocument({ alsoKnownAs: 'at://x', verificationMethods: { atproto: 7 } });
    expect(view.handles).toEqual([]);
    expect(view.signingKeys).toEqual([]);
    expect(view.unreadable).toEqual(['alsoKnownAs', 'verificationMethods.atproto']);
  });

  it('reads a document with no handle as having none', () => {
    expect(readDidDocument({}).handles).toEqual([]);
    expect(readDidDocument('not a map').unreadable).toEqual(['document']);
    expect(readDidDocument({ $map: [[1, 'x']] }).unreadable).toEqual(['document']);
  });
});

describe('splitHandle', () => {
  it('keeps the first label and the domain apart for the rose suffix', () => {
    expect(splitHandle('jler.web5.bbs.fans')).toEqual({ name: 'jler', suffix: '.web5.bbs.fans' });
    expect(splitHandle('localhost')).toEqual({ name: 'localhost', suffix: '' });
  });
});
