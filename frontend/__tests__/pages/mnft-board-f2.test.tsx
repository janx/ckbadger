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
import { ledeRestatements } from '@/__tests__/utils/lede-rule';
import { fullText } from '@/__tests__/utils/full-text';
import { expectPrompt, promptNote } from '@/__tests__/utils/prompt-heading';
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
    // 1,500, not 1,000 (readability R5, re-pinned on purpose): the cell's 1,000 CKB is "1,000" in
    // its
    // summary form, and the lede rule, which reads numbers without their units, took the class's
    // "1,000 planned" in the intro for a restatement of the metric.
    total: 1500,
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

  // Re-pinned on purpose (readability R8, lane p-assets; renamed, it was "opens with the
  // composition sentence, its life and the board properties"): the lede is its life, what it is and
  // whether it is live, then the composition; as a lede it names no count (the metrics row's, C7:
  // it read "then moved 2 times")
  it('opens with its life as the lede, then the composition sentence and the board properties (R8, OB-M4, OB-M5, OB-M6)', async () => {
    await page();
    const header = screen.getByRole('banner');
    expect(within(header).getByText('Centralized mix')).toBeInTheDocument();
    expect(header).toHaveTextContent('Partly off-chain: its content lives on a web server');
    const life = within(header).getByText(/^Issued by Issuer A to a UniPass lock/);
    expect(life).toHaveTextContent(
      /^Issued by Issuer A to a UniPass lock, then moved; the last move put it under .+, where it sits now\.$/
    );
    const lede = header.querySelector('[data-role="lede"]')!;
    expect(lede).toContainElement(life);
    expect(
      life.compareDocumentPosition(within(header).getByText('Centralized mix')) &
        Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();
    // Lane P: the ages are the sentence's hover, never its text.
    expect(within(header).getByText(/^Issued by Issuer A/)).toHaveAttribute(
      'title',
      expect.stringMatching(/^issued .+ ago · last moved .+ ago$/)
    );
    const props = within(header).getByLabelText('Properties');
    // re-pinned on purpose (lane P2's third round, item 8): tier 1 says the token in plain words a
    // reader who does not know the m-NFT script can read, "Number" and "Transfer" (it said the
    // script's "Token" and "State" with the state byte, "0x03 · claimed · locked"); the script's
    // terms stand once, in the evidence tier's permissions
    // re-pinned on purpose (lane P2's third round, item 1): the issuer is said once, where the
    // reader meets it first, the lede ("Issued by Issuer A …"); the properties' Issuer row and the
    // Class metric's "by …" said it again (the issuer's record stands in the evidence tier)
    expect([...props.querySelectorAll('dt')].map((d) => d.textContent)).toEqual([
      'Number',
      'Transfer',
      'Characteristic',
    ]);
    expect(props).toHaveTextContent('#99 of 200 issued · 1,500 planned');
    expect(props).toHaveTextContent('not allowed, the token is locked · claimed by its holder');
    // lane P2's third round (item 4): each value at one gap (12 px) after its label, left-aligned,
    // the label column the longest label's, "Characteristic", 14 characters at the page's 14 px
    // (8.4 px a glyph): 117.6, 118 px
    for (const row of props.querySelectorAll<HTMLElement>(':scope > div')) {
      expect(row).not.toHaveClass('justify-between');
      expect(row.style.gridTemplateColumns).toBe('118px minmax(0, 1fr)');
      expect(row.querySelector('dd')).not.toHaveClass('text-right');
    }
    expect(props).not.toHaveTextContent(/0x0\d|unclaimed|unlocked/);
    expect(screen.getByRole('region', { name: 'mNFT permissions' })).toHaveTextContent(
      /State 0x\d\d · claimed · locked/
    );
    expect(header).toHaveTextContent(/^.*Issued by Issuer A/);
    const metrics = screen.getByRole('region', { name: 'Metrics' });
    expect(metrics).not.toHaveTextContent(/by Issuer A|· to /);
    // re-pinned on purpose (the ink and size roles round): the issuer's name is a value at 13 px,
    // its label "Name ·" quiet at 12 beside it (one line, 12 px and quiet throughout before)
    const issuerName = screen.getByText(fullText('Name · Issuer A'));
    expect(issuerName.closest('[data-tier="2"]')).not.toBeNull();
    expect(issuerName).toHaveClass('text-text', 'text-small');
    expect(within(issuerName).getByText('Name ·')).toHaveClass('text-text-dim', 'text-meta');
    expect(screen.getByText('Live')).toHaveClass('text-jade', 'border-jade/40');
  });

  it('keeps the properties one column until md and never breaks a value inside a word', async () => {
    // Responsive review B#15: at 640 the 2×2 grid printed "8 b / ytes" and hung "planned".
    vi.mocked(api.getMnftItemDetail).mockResolvedValue({
      ...detail,
      characteristicHex: '0x0000000000000000',
    });
    await page();
    const props = within(screen.getByRole('banner')).getByLabelText('Properties');
    expect(props).toHaveClass('md:grid-cols-2');
    expect(props).not.toHaveClass('sm:grid-cols-2');
    // re-pinned on purpose (lane P2's third round, item 1): the third row now, the Issuer row gone
    // (the issuer is said once, in the lede)
    const characteristic = [...props.querySelectorAll('dd')][2];
    expect(characteristic).not.toHaveClass('break-all');
    expect(characteristic).toHaveTextContent('none, all zero · 8 bytes');
    expect(within(characteristic).getByText('8 bytes')).toHaveClass('whitespace-nowrap');
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
    // re-pinned on purpose (lane P2's third round, item 1): the asset pages' one pattern, the
    // section headed "History" with the sentence its versions give, no figure of its own (CKB held
    // and its split are the metric); it was "CKB held in its cell" headed "CKB held"
    const cell = screen.getByRole('region', { name: 'History' });
    const rows = within(cell).getAllByRole('article');
    expect(rows.map((r) => r.querySelector('a')!.textContent)).toEqual(['Issued', 'Moved', 'Now']);
    expect(cell).toHaveTextContent('1 CKB per byte');
    expect(cell.querySelector('[data-role="history-sentence"]')).not.toBeNull();
    const heading = within(cell).getByRole('heading', { name: 'History' });
    expectPrompt(heading, 'History');
    expect(heading.parentElement).not.toHaveTextContent(/occupied|free/);
    expect(heading).not.toHaveTextContent('1 CKB per byte');
    expect(heading.parentElement).toHaveTextContent('1 CKB per byte');
  });

  it('lists the board facts as the metrics row (OB-M8, C7) and the class block (OB-M9)', async () => {
    const { container } = await page();
    expect(screen.queryByRole('complementary', { name: 'Facts' })).toBeNull();
    const rail = screen.getByRole('region', { name: 'Metrics' });
    expect(ledeRestatements(container)).toEqual([]);
    for (const label of ['Owner', 'Class', 'Issued', 'Last moved', 'CKB held in its cell'])
      expect(within(rail).getByText(label)).toBeInTheDocument();
    expect(rail).toHaveTextContent('PW Lock · since');
    // re-pinned on purpose (lane P2's third round, item 1): the issuer is said once, in the lede;
    // the Class metric's note "by Issuer A" said it again
    expect(rail).not.toHaveTextContent('by Issuer A');
    expect(rail).toHaveTextContent('2 transfers since issue');
    const cls = screen.getByRole('region', { name: 'In Class A' });
    expect(cls).toHaveTextContent('200 issued of 1,500 · 198 live');
    // Phase 5 (A15): the class, run on its name; the link to it is the note, outside the name.
    const heading = within(cls).getByRole('heading', { name: 'Class Class A' });
    expectPrompt(heading, 'Class', 'Class A');
    expect(promptNote(heading)).toContainElement(
      within(cls).getByRole('link', { name: 'Open the class' })
    );
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
