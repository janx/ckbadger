import { render, screen, fireEvent, within } from '@/__tests__/utils/test-utils';
import Page from '@/app/classes/[classId]/client-page';
import { api } from '@/lib/api';
import { mockItemReads } from '@/__tests__/utils/item-read-fixtures';
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));
// An m-NFT class id is its issuer id (20 bytes) and a 4-byte class index (lane E draws it).
const mockClassId = '0x1111111111111111111111111111111111111111' + '00000003';
const mockCollection = {
  collectionId: mockClassId,
  standard: 'm-nft',
  name: 'Test mNFT Class',
  totalCount: 100,
  liveCount: 80,
  holdersCount: 25,
  activitiesCount: 50,
  ownedCapacity: '500000000000',
  ownedKnowledge: '300000000000',
  composition: {
    tier: 'unknown' as const,
    onchainCount: 0,
    pureCkbCount: 0,
    decentralizedMixtureCount: 0,
    centralizedMixtureCount: 0,
    unknownCount: 80,
    onchainRatio: '0',
  },
  classDetail: {
    classId: mockClassId,
    issuerId: '0x1111111111111111111111111111111111111111',
    name: 'Test mNFT Class',
    description: 'A test mNFT collection',
    renderer: null,
    total: 100,
    issued: 100,
    configure: 3,
    permissions: {
      allowClaim: false,
      allowLock: false,
      allowExtension: true,
      allowCharacteristicUpdate: true,
      allowTransferBeforeClaim: true,
      allowTransferAfterClaim: true,
      allowDestroyBeforeClaim: true,
      allowDestroyAfterClaim: true,
    },
  },
  issuerDetail: {
    issuerId: '0x1111111111111111111111111111111111111111',
    name: 'Test Issuer',
    classCount: 5,
    setCount: 2,
    infoHex: null,
  },
  createdAtBlock: 500000,
  ownerLockHash: '0x2222222222222222222222222222222222222222222222222222222222222222',
};

beforeEach(() => {
  mockItemReads();
  vi.mocked(api.getObjectCollection).mockResolvedValue(mockCollection);
});
it('keeps a loading state until the collection is read', () => {
  vi.mocked(api.getObjectCollection).mockReturnValue(new Promise(() => {}));
  render(<Page classId={mockClassId} />);
  expect(screen.getByRole('status')).toHaveTextContent('Loading collection');
});
it('reports read errors without generating a placeholder collection', async () => {
  vi.mocked(api.getObjectCollection).mockRejectedValue(new Error('Not found'));
  render(<Page classId={mockClassId} />);
  expect(await screen.findByRole('alert')).toHaveTextContent(
    'Could not load collection: Not found'
  );
});
it('renders exact class counts and configured permissions, not legacy bit meanings', async () => {
  render(<Page classId={mockClassId} />);
  await screen.findByRole('heading', { name: 'Test mNFT Class' });
  // C7 (Main.dc.html): the hero's figure — live objects, and how many of the minted are burned.
  expect(document.querySelector('[data-role="figure"]')).toHaveTextContent(
    'objects live80 of 100 minted · 20 burned'
  );
  expect(screen.getByRole('region', { name: 'mNFT permissions' })).toHaveTextContent('Claim');
  expect(screen.getByRole('region', { name: 'mNFT permissions' })).toHaveTextContent('Forbidden');
  expect(screen.queryByText('transferable, burnable')).not.toBeInTheDocument();
});
it('retains class and issuer raw evidence and separate full transaction history', async () => {
  render(<Page classId={mockClassId} />);
  // Lane E (C5): the class id byte by byte, the class's counts and its issuer's record.
  const evidence = await screen.findByRole('region', { name: 'The class id, byte by byte' });
  expect(
    [...evidence.querySelectorAll('[data-segment]')].map((s) => s.getAttribute('data-target'))
  ).toEqual(['field-0', 'field-1']);
  expect(within(evidence).getByText('Test Issuer · 5 classes · 2 sets')).toBeInTheDocument();
  expect(within(evidence).getByText('100 · planned 100')).toBeInTheDocument();
  expect(within(evidence).getByText('not recorded')).toBeInTheDocument();
  // Main.dc.html: "Recent activity" opens the full feed at its transactions.
  fireEvent.click(screen.getByRole('button', { name: /transactions →$/ }));
  expect(await screen.findByRole('table', { name: 'Asset transactions' })).toBeInTheDocument();
});
it('does not invent permissions or issuer data when class metadata is absent', async () => {
  const { classDetail: _class, issuerDetail: _issuer, ...collection } = mockCollection;
  vi.mocked(api.getObjectCollection).mockResolvedValue(collection);
  render(<Page classId={mockClassId} />);
  await screen.findByRole('heading', { name: 'Test mNFT Class' });
  expect(screen.queryByRole('region', { name: 'mNFT permissions' })).not.toBeInTheDocument();
  expect(screen.queryByText(/Test Issuer/)).not.toBeInTheDocument();
});
