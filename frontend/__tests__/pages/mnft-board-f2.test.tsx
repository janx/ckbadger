// F2 group `objects` (round 2): the m-NFT item page against the approved Mnft board — composition
// sentence and life summary (OB-M4, OB-M5), properties without configure decoding (OB-M6, moved to
// tier 2), "CKB held in its cell" over its owners (OB-M7), the facts rail (OB-M8), the class block
// (OB-M9), decoded token-id values in tier 2 (OB-M11), one Live style (OB-15) and folded batches
// in its feed (OB-M3).
import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  act,
  createTestQueryClient,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@/__tests__/utils/test-utils';
import MnftItemDetailPage from '@/app/objects/mnft/[objectId]/client-page';
import {
  api,
  ApiRequestError,
  type ItemProtocol,
  type MnftItemDetail,
  type ObjectCollection,
} from '@/lib/api';
import {
  mnftProtocolProducer,
  itemActivityFeedProducer,
  itemTransactionsProducer,
} from '@/__tests__/msw/fixtures/item-producer';

vi.mock('@/lib/api', async (load) => {
  const actual = await load<typeof import('@/lib/api')>();
  return {
    ...actual,
    api: {
      ...actual.api,
      getMnftItemDetail: vi.fn(),
      getObjectCollection: vi.fn(),
      getItemProtocol: vi.fn(),
      getItemCellVersions: vi.fn(),
      getItemActivityFeed: vi.fn(),
      getItemTransactions: vi.fn(),
    },
  };
});
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));

const id = mnftProtocolProducer.itemId;
const protocol = mnftProtocolProducer.versions[0];
const detail: MnftItemDetail = {
  nftId: id,
  standard: 'm-nft',
  isLive: true,
  ownerLockHash: protocol.cell.ownerLockHash,
  ownerAddress: protocol.cell.ownerAddress,
  createdAtBlock: protocol.cell.blockNumber,
  createdAtTx: protocol.cell.txHash,
  tokenIndex: 99,
  characteristicHex: protocol.decoded.characteristicHex,
  configure: protocol.decoded.configure,
  state: protocol.decoded.state,
  permissions: protocol.decoded.permissions,
  claimed: protocol.decoded.claimed,
  locked: protocol.decoded.locked,
  txHash: protocol.cell.txHash,
  outputIndex: protocol.cell.outputIndex,
  class: {
    classId: id.slice(0, 50),
    issuerId: id.slice(0, 42),
    name: 'Class A',
    description: 'Class description recorded on chain',
    renderer: 'https://example.com/renderer.png',
    total: 1000,
    issued: 200,
    configure: protocol.decoded.configure,
    permissions: protocol.decoded.permissions,
  },
  issuer: {
    issuerId: id.slice(0, 42),
    name: 'Issuer A',
    classCount: 2,
    setCount: 3,
    infoHex: '0x7b7d',
  },
  lifecycle: [],
  composition: {
    tier: 'centralized_mixture',
    onchainCount: 0,
    pureCkbCount: 0,
    decentralizedMixtureCount: 0,
    centralizedMixtureCount: 2,
    unknownCount: 0,
    onchainRatio: '0',
  },
};
const collection: ObjectCollection = {
  collectionId: detail.class.classId,
  standard: 'm-nft',
  name: 'Class A',
  totalCount: 200,
  liveCount: 198,
  holdersCount: 150,
  activitiesCount: 221,
  ownedCapacity: '32285294386739',
  ownedKnowledge: '32053400000000',
  composition: detail.composition,
};

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.getMnftItemDetail).mockResolvedValue(detail);
  vi.mocked(api.getItemProtocol).mockResolvedValue(mnftProtocolProducer);
  vi.mocked(api.getObjectCollection).mockResolvedValue(collection);
  vi.mocked(api.getItemActivityFeed).mockResolvedValue(itemActivityFeedProducer);
  vi.mocked(api.getItemTransactions).mockResolvedValue(itemTransactionsProducer);
});

function threeOwners(): Extract<ItemProtocol, { kind: 'm-nft' }> {
  const p: Extract<ItemProtocol, { kind: 'm-nft' }> = structuredClone(mnftProtocolProducer);
  const v = p.versions[0];
  const day = Date.UTC(2021, 11, 24, 0, 54, 0);
  const at = (i: number, lock: string, live: boolean) => {
    const c = structuredClone(v);
    c.cell.txHash = '0x' + String(i).repeat(64).slice(0, 64);
    c.cell.blockNumber = 6140203 + i * 1000;
    c.cell.timestamp = String(day + i * 86_400_000 * 300);
    c.cell.lockName = lock;
    c.cell.isLive = live;
    c.cell.consumedByTx = live
      ? null
      : '0x' +
        String(i + 1)
          .repeat(64)
          .slice(0, 64);
    c.cell.consumedAtBlock = live ? null : 6140203 + (i + 1) * 1000;
    return c;
  };
  p.versions = [at(1, 'UniPass', false), at(2, 'UniPass', false), at(3, 'PW Lock', true)];
  return p;
}

describe('Mnft board (F2 objects)', () => {
  beforeEach(() => {
    vi.mocked(api.getItemProtocol).mockResolvedValue(threeOwners());
  });
  async function page() {
    const view = render(<MnftItemDetailPage objectId={id} />);
    await screen.findByRole('heading', { name: 'Class A #99' });
    await screen.findByRole('region', { name: 'Raw item evidence' });
    return view;
  }

  it('opens with the composition sentence, its life and the board properties (OB-M4, OB-M5, OB-M6)', async () => {
    await page();
    const header = screen.getByRole('banner');
    expect(within(header).getByText('Centralized mix')).toBeInTheDocument();
    expect(header).toHaveTextContent('Partly off-chain: its content lives on a web server');
    expect(
      within(header).getByText(/^Issued by Issuer A .* ago to a UniPass lock/)
    ).toHaveTextContent('then moved 2 times');
    const props = within(header).getByLabelText('Properties');
    expect([...props.querySelectorAll('dt')].map((d) => d.textContent)).toEqual([
      'Token',
      'State',
      'Issuer',
      'Characteristic',
    ]);
    expect(props).toHaveTextContent('#99 of 200 issued · 1,000 planned');
    expect(props).toHaveTextContent('Issuer A · 2 classes');
    expect(screen.getByText('Live')).toHaveClass('text-jade', 'border-jade/40');
  });

  it('names where its image lives instead of a bare renderer card (OB-M4)', async () => {
    await page();
    const media = screen.getByRole('figure');
    expect(media).toHaveTextContent('Its image is a file on a web server');
    expect(media).toHaveTextContent('example.com/renderer.png');
    expect(within(media).getByRole('link', { name: 'Open the file ↗' })).toHaveAttribute(
      'href',
      'https://example.com/renderer.png'
    );
    expect(media).toHaveTextContent('If that server goes away, the picture goes with it');
  });

  it('moves the configure decoding to tier 2 (OB-M6)', async () => {
    const { container } = await page();
    const permissions = screen.getByRole('region', { name: 'mNFT permissions' });
    expect(permissions.closest('[data-tier]')).toHaveAttribute('data-tier', '2');
    expect(container.querySelector('[data-tier="1"]')).not.toHaveTextContent(
      'What its configuration permits'
    );
  });

  it('shows CKB held in its cell through its owners (OB-M7)', async () => {
    await page();
    const cell = screen.getByRole('region', { name: 'CKB held in its cell' });
    const rows = within(cell).getAllByRole('article');
    expect(rows.map((r) => r.querySelector('a')!.textContent)).toEqual(['Issued', 'Moved', 'Now']);
    expect(cell).toHaveTextContent('1 CKB per byte');
  });

  it('lists the board facts (OB-M8) and the class block (OB-M9)', async () => {
    await page();
    const rail = screen.getByRole('complementary', { name: 'Facts' });
    for (const label of ['Owner', 'Class', 'Issued', 'Last moved', 'CKB held in its cell'])
      expect(within(rail).getByText(label)).toBeInTheDocument();
    expect(rail).toHaveTextContent('PW Lock · since');
    expect(rail).toHaveTextContent('by Issuer A');
    expect(rail).toHaveTextContent('2 transfers since issue');
    const cls = screen.getByRole('region', { name: 'In Class A' });
    expect(cls).toHaveTextContent('200 issued of 1,000 · 198 live');
    expect(cls).toHaveTextContent('150 holders');
    expect(cls).toHaveTextContent('0 of 198 fully on-chain');
    expect(cls).toHaveTextContent('issued by Issuer A');
  });

  it('decodes the token id fields in tier 2 (OB-M11)', async () => {
    await page();
    const tokenId = screen.getByRole('heading', { name: 'm-NFT token ID' }).closest('section')!;
    expect(tokenId).toHaveTextContent(/Class index · big-endian = \d+/);
    expect(tokenId).toHaveTextContent(/Token index · big-endian = \d+/);
  });
});
