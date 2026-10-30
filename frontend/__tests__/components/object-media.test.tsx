import { render, screen } from '@/__tests__/utils/test-utils';
import { ObjectMedia } from '@/components/asset/object-media';
import type { SporeDetail } from '@/lib/api';
import { bitcoinTxHref, omigaDobHref, parseBtcfsUri } from '@/lib/btcfs';

// A Bitcoin txid shaped like the Nervape 3x one (0bfb…5719); the middle digits are a fixture.
const txid = '0bfb6dd6b0ac0fa8d8a9a0c8ad66d33c63cb34e08cf8ea76b8b6a1fbd2f15719';
const object: SporeDetail = {
  sporeId: '0x' + '12'.repeat(32),
  txHash: '0x' + '23'.repeat(32),
  outputIndex: 0,
  clusterId: null,
  contentType: 'image/png',
  contentSize: 1,
  ownerLockHash: null,
  isLive: true,
  createdAtBlock: 1,
  ownedCapacity: '6100000000',
  ownedKnowledge: '6100000000',
  typeScriptHash: '0x303e7b5e16db979e24e79690a8f3a3436cffd00e8c60b40e067db097136cc998',
  mediaProfile: {
    tier: 'btc_ckb',
    sources: [
      {
        uri: `btcfs://${txid}i0`,
        scheme: 'btcfs',
        sourceLocation: 'DOB trait',
        dependencyTier: 'btc_ckb',
      },
    ],
    issues: [],
  },
};
it('presents Bitcoin media as a copper link plate without fetching an image', () => {
  const { container } = render(
    <ObjectMedia
      object={object}
      contentHex="0x00"
      decoded={{
        status: 'decoded',
        sporeId: object.sporeId,
        contentType: 'dob/0',
        dnaHex: '0x00',
        traits: [],
        issues: [],
        media: [
          {
            mediaType: 'image/png',
            role: null,
            size: 1,
            hash: 'x',
            step: 0,
            url: 'https://external.example/image',
          },
        ],
      }}
    />
  );
  // OB-2: browsers cannot open btcfs://; the inscription is read on its Bitcoin transaction.
  const inscription = screen.getByRole('link', { name: 'Bitcoin tx 0bfb…5719 · inscription i0 ↗' });
  expect(inscription).toHaveAttribute('href', `https://mempool.space/tx/${txid}`);
  expect(inscription).toHaveAttribute('target', '_blank');
  expect(inscription).toHaveAttribute('rel', 'noopener noreferrer');
  expect(screen.getByText(/CKBadger reads CKB only/)).toBeInTheDocument();
  expect(screen.getByRole('link', { name: 'View on Omiga ↗' })).toHaveAttribute(
    'href',
    'https://omiga.io/info/dobs/0x303e7b5e16db979e24e79690a8f3a3436cffd00e8c60b40e067db097136cc998'
  );
  expect(container.querySelector('img')).toBeNull();
  expect(screen.getByText(/Its image lives on Bitcoin/)).toHaveClass('text-copper');
});
it('does not label pending composition as pure CKB', () => {
  render(
    <ObjectMedia
      object={{ ...object, mediaProfile: { tier: 'decode_pending', sources: [], issues: [] } }}
      contentHex={null}
    />
  );
  expect(screen.getByText(/awaiting DOB execution/)).toBeInTheDocument();
  expect(screen.queryByText('Pure CKB')).not.toBeInTheDocument();
});
it('parses BTCFS inscription URIs strictly (OB-2)', () => {
  expect(parseBtcfsUri(`btcfs://${txid}i0`)).toEqual({ txid, index: 0 });
  expect(parseBtcfsUri(`btcfs://${txid.toUpperCase()}i12`)).toEqual({ txid, index: 12 });
  for (const uri of [
    'btcfs://abc123i0',
    `btcfs://${txid}`,
    `btcfs://${txid}i`,
    `btcfs://${txid}i01`,
    `btcfs://${txid}i-1`,
    `btcfs://0x${txid}i0`,
    `ord://${txid}i0`,
    `btcfs://${txid}i0/extra`,
  ])
    expect(() => parseBtcfsUri(uri)).toThrow('Invalid BTCFS inscription URI');
  expect(bitcoinTxHref(txid)).toBe(`https://mempool.space/tx/${txid}`);
  expect(() => bitcoinTxHref(`0x${txid}`)).toThrow('Invalid Bitcoin txid');
  expect(() => omigaDobHref('0x1234')).toThrow('Invalid spore type script hash');
});
it('fails fast on a BTCFS source it cannot read instead of linking it', () => {
  const error = vi.spyOn(console, 'error').mockImplementation(() => {});
  expect(() =>
    render(
      <ObjectMedia
        object={{
          ...object,
          mediaProfile: {
            tier: 'btc_ckb',
            sources: [
              {
                uri: 'btcfs://abc123i0',
                scheme: 'btcfs',
                sourceLocation: 'DNA',
                dependencyTier: 'btc_ckb',
              },
            ],
            issues: [],
          },
        }}
        contentHex={null}
      />
    )
  ).toThrow('Invalid BTCFS inscription URI "btcfs://abc123i0"');
  error.mockRestore();
});
