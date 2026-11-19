import { QueryClientProvider } from '@tanstack/react-query';
import { render, screen, within, act, createTestQueryClient } from '@/__tests__/utils/test-utils';
import { SporeDetailView } from '@/components/asset/spore-detail';
import { CollectionDetail } from '@/components/asset/collection-detail';
import { CanonicalIdentityDetail } from '@/components/asset/identity-detail';
import { api, type ItemCellVersion } from '@/lib/api';
import { mockItemReads, dotcellItem } from '@/__tests__/utils/item-read-fixtures';
import {
  sporeDetailProducer,
  clusterDetailProducer,
  clusterProtocolProducer,
  dotcellProtocolProducer,
  mnftCollectionInsightsProducer,
} from '@/__tests__/msw/fixtures/item-producer';
import { fullText } from '@/__tests__/utils/full-text';
vi.mock('@/components/layout/header', () => ({ Header: () => null }));
beforeEach(mockItemReads);
it('keeps object capacity primary, one versions heading and exact occupied values', async () => {
  render(<SporeDetailView object={sporeDetailProducer} error={null} />);
  // F2 objects (OB-9): the versions are the board's "CKB held in its cell" — one heading for both.
  expect(await screen.findByLabelText(/cell versions$/)).toBeInTheDocument();
  expect(screen.queryByRole('heading', { name: /^Cell versions/ })).not.toBeInTheDocument();
  // re-pinned on purpose (lane P2's third round, item 1): the object page takes the asset pages' one pattern, the metric states CKB held and its split, and the section is headed "History" with its sentence and its versions, no display figure of its own (the capacity partition said the metric again)
  expect(screen.queryByRole('heading', { name: 'CKB held' })).toBeNull();
  const history = screen.getByRole('region', { name: 'History' });
  const heading = within(history).getByRole('heading', { name: 'History' });
  expect(heading.parentElement).toHaveTextContent('1 CKB per byte');
  expect(heading.parentElement).not.toHaveTextContent(/occupied|free/);
  expect(
    screen.queryByLabelText(`Object ${sporeDetailProducer.sporeId} capacity partition`)
  ).toBeNull();
  // the sentence computed from its points stands first in the section, before the versions
  const sentence = history.querySelector('[data-role="history-sentence"]')!;
  expect(
    sentence.compareDocumentPosition(screen.getByLabelText(/cell versions$/)) &
      Node.DOCUMENT_POSITION_FOLLOWING
  ).toBeTruthy();
  expect(screen.getByRole('region', { name: 'Metrics' })).toHaveTextContent(
    /CKB held in its cell.+229 occupied · 271 free/
  );
  expect(
    screen.getByRole('region', { name: 'Raw item evidence' }).closest('[data-tier]')
  ).toHaveAttribute('data-tier', '2');
});
// A4 Chrome F4 (the other tier-2 grids): the raw evidence's out point set its tx hash at 13 px in
// an 11 px line, 514.8 px, in a 460 px column at 1024 ((976 − 56) / 2): a scroll of 55 px. The hash
// takes its line's 11 px: 66 × 6.6 = 435.6 px and ":N" after it, 448.8 px, within 460.
// Re-pinned on purpose (lane P2's follow-up, item 8; 48da6a17 left it red, "expect(element)
// .toHaveClass("text-meta")"): the out point's hash is a head's id (HeadId) in the 11 px line, its
// index said in words before it; at 11 px it is 435.6 px, one line in the 460 px column at 1024,
// two halves below 435.6
// Re-pinned on purpose (the ink and size roles round): the out point is a link, at 13 px; its label
// stays at 12 beside it. At 13 px its hash is 66 × 7.8 = 514.8 px: one line in the tier's 588 px
// column at 1280, two halves of 257.4 px from 1024 (460) down, never a scroll (a head's id).
it('sets the raw evidence’s out point as a link at 13 px, its label at 12', async () => {
  render(<SporeDetailView object={sporeDetailProducer} error={null} />);
  const evidence = await screen.findByRole('region', { name: 'Raw item evidence' });
  const label = within(evidence).getByText(/^Outpoint · output \d+ of$/);
  expect(label).toHaveClass('text-meta', 'text-text-dim');
  const line = label.parentElement!;
  expect(line).toHaveClass('text-small');
  const hash = line.querySelector('[data-head-id]') as HTMLElement;
  expect(hash).not.toBeNull();
  expect(hash.className).not.toMatch(/(^|\s)text-(body|meta|section)/);
});

it('rejects impossible object capacity rather than clamping it', () => {
  const log = vi.spyOn(console, 'error').mockImplementation(() => {});
  expect(() =>
    render(<SporeDetailView object={{ ...sporeDetailProducer, ownedCapacity: '1' }} error={null} />)
  ).toThrow('occupied capacity exceeds');
  log.mockRestore();
});
it('does not identify cluster metadata ownership as a current cell without canonical proof', async () => {
  render(<CollectionDetail id={clusterDetailProducer.clusterId} kind="spore" />);
  await screen.findByRole('heading', { name: 'Genes' });
  expect(await screen.findByText('No current cluster cell')).toBeInTheDocument();
  expect(screen.getByRole('region', { name: 'Cluster cell evidence' })).toHaveTextContent(
    'No current cell carries this cluster type.'
  );
});
it('links a canonically resolved cluster cell through the one-segment outpoint route', async () => {
  vi.mocked(api.getSporeClusterProtocol).mockResolvedValue(clusterProtocolProducer);
  render(<CollectionDetail id={clusterDetailProducer.clusterId} kind="spore" />);
  // Lane E (C5): the out point in the cell's "live cell" row is the link (was "Current cluster cell →").
  const cell = await screen.findByRole('region', { name: 'The cluster cell, byte by byte' });
  const link = within(within(cell).getByLabelText('The cluster cell')).getAllByRole('link')[0];
  expect(link).toHaveAttribute(
    'href',
    `/mainnet/cell/${clusterProtocolProducer.current.cell.txHash}-${clusterProtocolProducer.current.cell.outputIndex}`
  );
});
it('retains collection evidence and reports a failed background refresh', async () => {
  const client = createTestQueryClient();
  render(
    <QueryClientProvider client={client}>
      <CollectionDetail id={clusterDetailProducer.clusterId} kind="spore" />
    </QueryClientProvider>
  );
  await screen.findByRole('heading', { name: 'Genes' });
  vi.mocked(api.getObjectCollection).mockRejectedValue(new Error('secondary failed'));
  await act(async () => {
    await client.invalidateQueries({
      queryKey: ['object-collection', clusterDetailProducer.clusterId],
    });
  });
  expect(
    await screen.findByText(/Could not refresh collection: secondary failed/)
  ).toBeInTheDocument();
  // C7: the retained collection still shows its CKB held in the hero (no rail any more).
  // the hero's figure in the summary form (readability R5, re-pinned on purpose: was
  // "4,000.00000000")
  expect(document.querySelector('[data-role="hero"]')).toHaveTextContent(
    '4,000 CKB held in their cells'
  );
});
it('shows an m-NFT class its real holder buckets and no DOB blueprint line (#3)', async () => {
  vi.mocked(api.getCollectionInsights).mockResolvedValue(mnftCollectionInsightsProducer);
  render(<CollectionDetail id={'0x' + '8f'.repeat(24)} kind="object" />);
  expect(await screen.findByText('Full collection: 2 holders · 8 items')).toBeInTheDocument();
  expect(api.getCollectionInsights).toHaveBeenCalledWith('0x' + '8f'.repeat(24), 'object');
  expect(screen.queryByText('No DOB blueprint in this collection.')).not.toBeInTheDocument();
  expect(screen.queryByText('No current holders.')).not.toBeInTheDocument();
});
it('reports failed m-NFT insights instead of drawing an empty weekly panel (#17)', async () => {
  vi.mocked(api.getCollectionInsights).mockRejectedValue(new Error('insights unavailable'));
  render(<CollectionDetail id={'0x' + '8f'.repeat(24)} kind="object" />);
  expect(
    await screen.findByText(/Could not load collection insights: insights unavailable/)
  ).toBeInTheDocument();
  expect(
    await screen.findByText('Weekly transfers unavailable: insights unavailable')
  ).toBeInTheDocument();
});
it('draws an identity collection its weekly transfers from the insights it is served (#9)', async () => {
  vi.mocked(api.getCollectionInsights).mockResolvedValue({
    ...mnftCollectionInsightsProducer,
    weeklyActivity: [
      {
        weekStart: '2023-11-08',
        mintTransactions: 0,
        transferTransactions: 7,
        burnTransactions: 0,
      },
    ],
  });
  render(<CollectionDetail id="dotbit" kind="identity" />);
  expect(await screen.findByText(fullText(/Week of 2023-11-08: 7 transfers/))).toBeInTheDocument();
  expect(api.getCollectionInsights).toHaveBeenCalledWith('dotbit', 'identity');
  expect(screen.queryByText(/not indexed for identity collections/)).not.toBeInTheDocument();
  // Lane E: an identity collection has no developer evidence; no empty tier 2 (was an eyebrow and
  // an empty "Collection metadata" heading).
  expect(document.querySelector('[data-tier="2"]')).toBeNull();
});
it('separates physical cell ownership from protocol-named .cell parties', async () => {
  const p = dotcellProtocolProducer;
  render(
    <CanonicalIdentityDetail
      kind="dotcell"
      id={p.itemId}
      title="alice.cell"
      isLive
      createdAtBlock={p.versions[0].cell.blockNumber}
      error={null}
      dotcell={dotcellItem()}
    />
  );
  const facts = await screen.findByRole('region', { name: 'Metrics' });
  // Phase 5 (D13): the identity's versions run on their count.
  await screen.findByRole('heading', { name: `Cell versions ${p.versions.length}` });
  // F2 identities (round 2, ID-DC4 / ID-DC7): the rail names the protocol owner and manager;
  // the physical lock every name shares is tier-2 evidence, explained there.
  expect(facts).toHaveTextContent(p.versions[0].decoded.owner.hashPrefix);
  expect(facts).toHaveTextContent(p.versions[0].decoded.manager.hashPrefix);
  expect(
    facts.querySelector(`a[href='/mainnet/address/${p.versions[0].cell.ownerAddress}']`)
  ).toBeNull();
  const cell = screen.getByRole('region', { name: 'Name cell' });
  expect(
    cell.querySelector(`a[href='/mainnet/address/${p.versions[0].cell.ownerAddress}']`)
  ).not.toBeNull();
  expect(cell).toHaveTextContent('ownership is the owner field in the data');
});
it("titles collection feed rows from the API's collectionContext, never the tx's first item (OB-C13)", async () => {
  render(<CollectionDetail id={clusterDetailProducer.clusterId} kind="spore" />);
  // Main.dc.html: the cluster's "Recent activity" rows, then its full feed's Transactions tab.
  const recent = await screen.findByRole('region', { name: 'Recent activity' });
  expect(await within(recent).findByText('Transferred 1')).toBeInTheDocument();
  expect(within(recent).queryByText(/^Object (mint|transfer|burn)$/)).toBeNull();
  // re-pinned on purpose (lane P2's follow-up, item 10): the fold's one control, "all N
  // transactions ↓" under the rows (it was "N transactions →" in the heading's note)
  act(() => screen.getByRole('button', { name: /^all \S+\stransactions ↓$/ }).click());
  const feed = await screen.findByRole('region', { name: 'What happened' });
  // The Transactions tab says the same thing per row.
  // Table and compact layouts both carry it.
  expect((await within(feed).findAllByText('Minted 1')).length).toBeGreaterThan(0);
});

// Lane P2's fifth round (item 2, MUST): a melted Spore's History said "The object's cell holds 320
// CKB" under "No live cell holds capacity for this burned object." It says what its last cell held
// when it was consumed, in the past tense.
it('says what a melted object’s last cell held, in the past tense, under its History', async () => {
  const live = (await api.getItemCellVersions('spore', sporeDetailProducer.sporeId)).data;
  const consumed: ItemCellVersion[] = live.map((v) => ({
    ...v,
    isLive: false,
    consumedByTx: v.consumedByTx ?? `0x${'56'.repeat(32)}`,
    consumedAtBlock: v.consumedAtBlock ?? v.blockNumber + 1,
  }));
  vi.mocked(api.getItemCellVersions).mockResolvedValue({ data: consumed, total: consumed.length });
  render(<SporeDetailView object={{ ...sporeDetailProducer, isLive: false }} error={null} />);
  const history = await screen.findByRole('region', { name: 'History' });
  await screen.findByLabelText(/cell versions$/);
  const sentence = history.querySelector('[data-role="history-sentence"]')!;
  expect(history).toHaveTextContent('No live cell holds capacity for this burned object.');
  expect(sentence.textContent).toMatch(
    /^The object’s (last )?cell held .+ CKB.* when it was consumed\./
  );
  expect(sentence.textContent).not.toMatch(/holds/);
});
