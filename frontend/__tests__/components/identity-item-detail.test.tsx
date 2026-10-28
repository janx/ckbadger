import { render, screen, fireEvent, waitFor } from '@/__tests__/utils/test-utils';
import {
  IdentityItemDetail,
  type IdentityItemDetailConfig,
} from '@/components/identity/identity-item-detail';
import { api, ApiRequestError } from '@/lib/api';
import { mockItemReads, identityItem, emptyPage } from '@/__tests__/utils/item-read-fixtures';
import {
  dotbitV4ProtocolProducer,
  didProtocolProducer,
} from '@/__tests__/msw/fixtures/item-producer';
vi.mock('@/components/layout/header', () => ({ Header: () => null }));
const mockFetchDetail = vi.fn(),
  mockFetchActivities = vi.fn();
const dotbitConfig: IdentityItemDetailConfig = {
  standard: 'dotbit',
  fetchDetail: mockFetchDetail,
  detailEndpoint: (id) => `/assets/identities/dotbit/items/${encodeURIComponent(id)}`,
  fetchActivities: mockFetchActivities,
  labels: {
    standardDisplay: 'DOTBIT',
    nameLabel: '.bit Name',
    idLabel: 'Account ID',
    backLabel: 'Back to .bit Collection',
    backHref: '/identities/dotbit',
    defaultTitle: '.bit account',
    notFoundMsg: '.bit item not found',
    recycledMsg: 'Recycled .bit account has no live cell.',
    showExpiry: true,
  },
};

const didCkbConfig: IdentityItemDetailConfig = {
  standard: 'did_ckb',
  fetchDetail: mockFetchDetail,
  detailEndpoint: (id) => `/assets/identities/did/items/${encodeURIComponent(id)}`,
  fetchActivities: mockFetchActivities,
  labels: {
    standardDisplay: 'DID:CKB',
    nameLabel: 'did:ckb Name',
    idLabel: 'DID ID',
    backLabel: 'Back to did:ckb Collection',
    backHref: '/identities/did:ckb',
    defaultTitle: 'did:ckb identity',
    notFoundMsg: 'did:ckb item not found',
    recycledMsg: 'Recycled did:ckb identity has no live cell.',
    showExpiry: false,
  },
};

beforeEach(() => {
  mockItemReads();
  mockFetchDetail.mockReset();
  mockFetchDetail.mockResolvedValue(identityItem('dotbit'));
});
it('uses the canonical item outpoint and protocol instead of resolving an owner balance', async () => {
  render(<IdentityItemDetail config={dotbitConfig} identityId={dotbitV4ProtocolProducer.itemId} />);
  await screen.findByRole('region', { name: 'Raw item evidence' });
  expect(screen.getByRole('heading', { level: 1, name: 'alice.bit' })).toBeInTheDocument();
  const cell = dotbitV4ProtocolProducer.versions[0].cell;
  expect(
    screen
      .getAllByRole('link')
      .some((a) => a.getAttribute('href') === `/mainnet/cell/${cell.txHash}/${cell.outputIndex}`)
  ).toBe(true);
  expect(api.getItemProtocol).toHaveBeenCalledWith('dotbit', dotbitV4ProtocolProducer.itemId);
});
it('supports did:ckb document identity without fabricating expiry fields', async () => {
  mockFetchDetail.mockResolvedValue(identityItem('did'));
  render(<IdentityItemDetail config={didCkbConfig} identityId={didProtocolProducer.itemId} />);
  expect(await screen.findByRole('region', { name: 'DID document' })).toBeInTheDocument();
  expect(screen.queryByText(/expires/i)).not.toBeInTheDocument();
});
it('retains configuration-specific not-found semantics', async () => {
  mockFetchDetail.mockRejectedValue(new ApiRequestError(404, 'NOT_FOUND', 'missing'));
  const view = render(<IdentityItemDetail config={dotbitConfig} identityId="0xabc" />);
  expect(await screen.findByText('/assets/identities/dotbit/items/0xabc')).toBeInTheDocument();
  expect(
    screen.getByText('This explorer could not open the identity at this address.')
  ).toBeInTheDocument();
  view.unmount();
  render(<IdentityItemDetail config={didCkbConfig} identityId="0xabc" />);
  expect(await screen.findByText('/assets/identities/did/items/0xabc')).toBeInTheDocument();
});
it('resets the independent activity cursor when navigating to another identity', async () => {
  const first = identityItem('dotbit'),
    second = { ...first, nftId: '0x' + 'ee'.repeat(20), name: 'bob.bit' };
  mockFetchDetail.mockImplementation(async (id: string) => (id === first.nftId ? first : second));
  vi.mocked(api.getItemActivityFeed).mockResolvedValue({
    ...emptyPage(),
    hasMore: true,
    nextCursor: 'older',
  });
  const view = render(<IdentityItemDetail config={dotbitConfig} identityId={first.nftId} />);
  fireEvent.click(await screen.findByRole('button', { name: 'Next' }));
  await waitFor(() =>
    expect(api.getItemActivityFeed).toHaveBeenLastCalledWith('dotbit', first.nftId, {
      limit: 50,
      cursor: 'older',
    })
  );
  view.rerender(<IdentityItemDetail config={dotbitConfig} identityId={second.nftId} />);
  await waitFor(() =>
    expect(api.getItemActivityFeed).toHaveBeenLastCalledWith('dotbit', second.nftId, {
      limit: 50,
      cursor: undefined,
    })
  );
});
it('keeps recycled identities inspectable without claiming a live cell', async () => {
  mockFetchDetail.mockResolvedValue({
    ...identityItem('dotbit'),
    isLive: false,
    ownerLockHash: null,
  });
  render(<IdentityItemDetail config={dotbitConfig} identityId={dotbitV4ProtocolProducer.itemId} />);
  expect(await screen.findByText('Recycled / inactive')).toBeInTheDocument();
  expect(await screen.findByRole('region', { name: 'Raw item evidence' })).toBeInTheDocument();
});
