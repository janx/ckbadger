import { Component, type ReactNode } from 'react';
import { QueryClientProvider } from '@tanstack/react-query';
import {
  render,
  screen,
  fireEvent,
  waitFor,
  act,
  createTestQueryClient,
} from '@/__tests__/utils/test-utils';
import SporeDetailPage from '@/app/objects/[sporeId]/client-page';
import { api, ApiRequestError, type ItemCellVersions } from '@/lib/api';
import { mockItemReads, emptyPage } from '@/__tests__/utils/item-read-fixtures';
import {
  sporeDetailProducer,
  sporeCellVersionsProducer,
  objectGeneticsProducer,
} from '@/__tests__/msw/fixtures/item-producer';
vi.mock('@/components/layout/header', () => ({ Header: () => null }));
const replace = vi.fn();
vi.mock('@/src/navigation', () => ({
  useRouter: () => ({ replace }),
  usePathname: () => '/objects/test',
  useSearchParams: () => new URLSearchParams(),
}));
class Boundary extends Component<{ children: ReactNode }, { error: string | null }> {
  state = { error: null as string | null };
  static getDerivedStateFromError(error: Error) {
    return { error: error.message };
  }
  render() {
    return this.state.error ? <p role="alert">{this.state.error}</p> : this.props.children;
  }
}
const id = sporeDetailProducer.sporeId;
beforeEach(() => {
  mockItemReads();
  replace.mockClear();
});
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});
it('renders exact held/occupied/free capacity and canonical cell versions before whole-life feeds', async () => {
  const { container } = render(<SporeDetailPage sporeId={id} />);
  expect(
    await screen.findByRole('heading', { level: 1, name: 'Genes' }, { timeout: 5000 })
  ).toHaveClass('text-[26px]');
  expect(await screen.findByLabelText('Genes cell versions')).toHaveTextContent('500.00000000');
  expect(screen.getByLabelText(`Object ${id} capacity partition`)).toHaveTextContent(
    '229.00000000'
  );
  expect(screen.getByLabelText(`Object ${id} capacity partition`)).toHaveTextContent(
    '271.00000000'
  );
  expect(container.querySelector('[data-tier="2"]')).toHaveClass('mt-[72px]');
  expect(api.getItemCellVersions).toHaveBeenCalledWith('spore', id);
});
it('renders exact raw protocol bytes and current gene mapping with separate VM provenance', async () => {
  render(<SporeDetailPage sporeId={id} />);
  expect(await screen.findByLabelText('DNA and expression')).toHaveTextContent(
    '18446744073709551614'
  );
  expect(screen.getByLabelText('DNA and expression')).toHaveTextContent(
    'original decoder binary hash was not recorded'
  );
  expect(await screen.findByLabelText('Raw item evidence')).toHaveTextContent('spore cell data');
  expect(api.getItemProtocol).toHaveBeenCalledWith('spore', id);
});
it('retains full long content type and never links absent ownership to a null address', async () => {
  const contentType = 'text/' + 'long-subtype-'.repeat(20);
  vi.mocked(api.getSporeObject).mockResolvedValue({
    ...sporeDetailProducer,
    contentType,
    ownerLockHash: null,
    ownerAddress: null,
  });
  const { container } = render(<SporeDetailPage sporeId={id} />);
  expect(await screen.findByText(contentType + ' · 38 bytes')).toHaveClass('break-all');
  expect(screen.getByText('No live owner')).toBeInTheDocument();
  expect(container.querySelector('a[href*="/null"]')).toBeNull();
  expect(api.getObjectGenetics).not.toHaveBeenCalled();
});
it('shows consumed capacity as exact zero and retains its final historical cell', async () => {
  vi.mocked(api.getSporeObject).mockResolvedValue({
    ...sporeDetailProducer,
    isLive: false,
    ownerLockHash: null,
    ownedCapacity: '0',
    ownedKnowledge: '0',
  });
  const history: ItemCellVersions = structuredClone(sporeCellVersionsProducer);
  history.data[0].isLive = false;
  history.data[0].consumedByTx = '0x' + 'aa'.repeat(32);
  history.data[0].consumedAtBlock = 101;
  vi.mocked(api.getItemCellVersions).mockResolvedValue(history);
  render(<SporeDetailPage sporeId={id} />);
  expect(await screen.findByText('Melted')).toBeInTheDocument();
  expect(screen.getByText(/No live cell holds capacity/)).toBeInTheDocument();
  expect(screen.getByLabelText(`Object ${id} capacity partition`)).toHaveTextContent('0.00000000');
  expect(await screen.findByLabelText('Genes cell versions')).toHaveTextContent('500.00000000');
});
it('fails with item context when capacity violates the occupied invariant', async () => {
  vi.mocked(api.getSporeObject).mockResolvedValue({ ...sporeDetailProducer, ownedCapacity: '1' });
  const error = vi.spyOn(console, 'error').mockImplementation(() => {});
  render(
    <Boundary>
      <SporeDetailPage sporeId={id} />
    </Boundary>
  );
  expect(await screen.findByRole('alert')).toHaveTextContent(
    `${id}: occupied capacity exceeds held capacity`
  );
  error.mockRestore();
});
it('presents BTCFS as link-only media even when a decoded image URL exists', async () => {
  vi.mocked(api.getSporeObject).mockResolvedValue({
    ...sporeDetailProducer,
    mediaProfile: {
      tier: 'btc_ckb',
      sources: [
        {
          uri: `btcfs://${'0bfb'.padEnd(60, '0')}5719i0`,
          scheme: 'btcfs',
          sourceLocation: 'DNA',
          dependencyTier: 'btc_ckb',
        },
      ],
      issues: [],
    },
  });
  const { container } = render(<SporeDetailPage sporeId={id} />);
  expect(
    await screen.findByRole('link', { name: 'Bitcoin tx 0bfb…5719 · inscription i0 ↗' })
  ).toHaveAttribute('href', `https://mempool.space/tx/${'0bfb'.padEnd(60, '0')}5719`);
  expect(screen.getByRole('link', { name: 'View on Omiga ↗' })).toHaveAttribute(
    'href',
    `https://omiga.io/info/dobs/${sporeDetailProducer.typeScriptHash}`
  );
  expect(container.querySelector('img')).toBeNull();
});
it('does not repair unavailable raw data with transaction/getCell or client-side protocol parsing', async () => {
  vi.mocked(api.getItemProtocol).mockRejectedValue(new Error('local CKB reader unavailable'));
  const tx = vi.spyOn(api, 'getTransactionDetail'),
    cell = vi.spyOn(api, 'getCell');
  render(<SporeDetailPage sporeId={id} />);
  expect(
    (await screen.findAllByRole('alert')).some((el) =>
      el.textContent?.includes('local CKB reader unavailable')
    )
  ).toBe(true);
  expect(tx).not.toHaveBeenCalled();
  expect(cell).not.toHaveBeenCalled();
});
it('refreshes media classification when pending VM work completes, then stops polling', async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  const complete = {
    status: 'decoded',
    sporeId: id,
    contentType: 'dob/0',
    dnaHex: '0x01',
    traits: [{ name: 'Face', value: 'Blue' }],
    media: [],
    issues: [],
  };
  vi.mocked(api.getSporeObjectDecoded)
    .mockResolvedValueOnce({ ...complete, status: 'pending' })
    .mockResolvedValue(complete);
  vi.mocked(api.getSporeObject)
    .mockResolvedValueOnce({
      ...sporeDetailProducer,
      mediaProfile: { tier: 'decode_pending', sources: [], issues: [] },
    })
    .mockResolvedValue({
      ...sporeDetailProducer,
      mediaProfile: { tier: 'pure_ckb', sources: [], issues: [] },
    });
  vi.mocked(api.getObjectGenetics).mockResolvedValue({
    ...objectGeneticsProducer,
    cachedVmTraits: {
      ...objectGeneticsProducer.cachedVmTraits,
      status: 'pending',
      traits: [],
      decodedAt: null,
    },
  });
  render(<SporeDetailPage sporeId={id} />);
  expect((await screen.findAllByText(/Decode pending/)).length).toBeGreaterThan(0);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(2100);
  });
  await waitFor(() => expect(api.getSporeObject).toHaveBeenCalledTimes(2));
  const count = vi.mocked(api.getSporeObjectDecoded).mock.calls.length;
  await act(async () => {
    await vi.advanceTimersByTimeAsync(5000);
  });
  expect(api.getSporeObjectDecoded).toHaveBeenCalledTimes(count);
});
it('shows failed VM reason without fabricating traits', async () => {
  vi.mocked(api.getObjectGenetics).mockResolvedValue({
    ...objectGeneticsProducer,
    cachedVmTraits: {
      ...objectGeneticsProducer.cachedVmTraits,
      status: 'failed',
      decodedAt: null,
      traits: [],
      reason: 'VM exit 3',
    },
  });
  render(<SporeDetailPage sporeId={id} />);
  expect(await screen.findByText(/VM exit 3/)).toBeInTheDocument();
  expect(screen.queryByLabelText('Cached VM traits')).not.toBeInTheDocument();
});
it('keeps object data visible with a background refresh error', async () => {
  const client = createTestQueryClient();
  render(
    <QueryClientProvider client={client}>
      <SporeDetailPage sporeId={id} />
    </QueryClientProvider>
  );
  await screen.findByRole('heading', { level: 1, name: 'Genes' });
  vi.mocked(api.getSporeObject).mockRejectedValue(new Error('secondary read failed'));
  await act(async () => {
    await client.invalidateQueries({ queryKey: ['spore', id], exact: true });
  });
  expect(await screen.findByText(/Could not refresh object/)).toHaveTextContent(
    'Showing previously loaded data'
  );
  expect(screen.getByLabelText(`Object ${id} capacity partition`)).toHaveTextContent(
    '500.00000000'
  );
});
it('switches to real transaction layer and requests its own full scope', async () => {
  vi.mocked(api.getItemTransactions).mockResolvedValue(emptyPage());
  render(<SporeDetailPage sporeId={id} />);
  await screen.findByRole('tab', { name: 'Transactions' });
  fireEvent.click(screen.getByRole('tab', { name: 'Transactions' }));
  expect(await screen.findByText('No transactions.')).toBeInTheDocument();
  expect(api.getItemTransactions).toHaveBeenCalledWith('spore', id, {
    limit: 50,
    cursor: undefined,
  });
});
it.each(['dotbit', 'did:ckb', 'dotcell', '0x' + 'aa'.repeat(24)])(
  'routes known non-Spore identifier %s without probing object endpoints',
  async (value) => {
    render(<SporeDetailPage sporeId={value} />);
    await waitFor(() => expect(replace).toHaveBeenCalled());
    expect(api.getSporeObject).not.toHaveBeenCalled();
    expect(api.getObjectCollection).not.toHaveBeenCalled();
  }
);
it('only a real object 404 permits probing the collection namespace', async () => {
  vi.mocked(api.getSporeObject).mockRejectedValue(
    new ApiRequestError(404, 'not_found', 'Spore not found')
  );
  render(<SporeDetailPage sporeId={id} />);
  expect(await screen.findByRole('heading', { level: 1, name: 'Genes' })).toBeInTheDocument();
  expect(api.getObjectCollection).toHaveBeenCalledWith(id);
});
it('does not turn a corrupt object read into an alternative collection lookup', async () => {
  vi.mocked(api.getSporeObject).mockRejectedValue(new Error('corrupt cell'));
  render(<SporeDetailPage sporeId={id} />);
  expect(await screen.findByRole('alert')).toHaveTextContent('corrupt cell');
  expect(api.getObjectCollection).not.toHaveBeenCalled();
});
it('renders not found for both absent namespaces or an unrecognized identifier', async () => {
  vi.mocked(api.getSporeObject).mockRejectedValue(
    new ApiRequestError(404, 'not_found', 'Spore not found')
  );
  vi.mocked(api.getObjectCollection).mockRejectedValue(
    new ApiRequestError(404, 'not_found', 'Collection not found')
  );
  const { rerender } = render(<SporeDetailPage sporeId={id} />);
  expect(await screen.findByText(`/assets/objects/${id}`)).toBeInTheDocument();
  expect(screen.getByText(/Both \/spore\/objects\/.* returned HTTP 404/)).toBeInTheDocument();
  rerender(<SporeDetailPage sporeId="not-an-id" />);
  expect(screen.getByRole('heading', { name: 'Invalid object identifier' })).toBeInTheDocument();
});
