import { render, screen } from '@/__tests__/utils/test-utils';
import { ObjectMedia } from '@/components/asset/object-media';
import type { SporeDetail } from '@/lib/api';
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
  mediaProfile: {
    tier: 'btc_ckb',
    sources: [
      {
        uri: 'btcfs://abc123i0',
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
  expect(screen.getByRole('link', { name: 'Open the inscription ↗' })).toHaveAttribute(
    'href',
    'btcfs://abc123i0'
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
