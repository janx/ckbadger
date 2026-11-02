import { fireEvent, render, screen, within } from '@/__tests__/utils/test-utils';
import {
  ClusterCellEvidence,
  MnftClassEvidence,
  StoredDescription,
} from '@/components/asset/collection-evidence';
import { clusterProtocolProducer } from '@/__tests__/msw/fixtures/item-producer';

// Consistency lane E (C5): the collection pages' tier 2 — the cluster cell byte by byte with its
// place, holder and type; the stored description folded; an m-NFT class id byte by byte.
describe('ClusterCellEvidence', () => {
  it('draws the cluster cell to scale and names the molecule header the bytes prove', () => {
    const { current, clusterId } = clusterProtocolProducer;
    const { container } = render(<ClusterCellEvidence clusterId={clusterId} current={current} />);
    const section = screen.getByRole('region', { name: 'The cluster cell, byte by byte' });
    expect(
      [...container.querySelectorAll('[data-segment]')].map((s) => s.getAttribute('data-target'))
    ).toEqual(['gap:0-16', 'field-0', 'field-1', 'field-2']);
    fireEvent.mouseEnter(screen.getByRole('button', { name: /^\[0\.\.16\)/ }));
    const detail = screen.getByRole('complementary', {
      name: 'The cluster cell, byte by byte field detail',
    });
    expect(within(detail).getByText('molecule table header')).toBeInTheDocument();
    fireEvent.mouseEnter(screen.getByRole('button', { name: /^name · / }));
    expect(detail).toHaveTextContent(
      'The cluster’s name. A 4-byte length (5), then 5 bytes of UTF-8 text.'
    );
    const facts = within(section).getByLabelText('The cluster cell');
    expect(facts).toHaveTextContent(
      'live cell0xa5c534…3261:0 · created in #99 · 2023-11-15held byckb1qpq5zs2p…utsyjq · Unregistered locktype0x7366a615…030075 · the cluster id is this cell’s type args'
    );
    expect(within(facts).getAllByRole('link')[0]).toHaveAttribute(
      'href',
      `/mainnet/cell/${current.cell.txHash}-0`
    );
  });
});

describe('StoredDescription', () => {
  it('folds a description that is not JSON exactly as stored', () => {
    render(<StoredDescription description={'Plain words, {not json'} />);
    const details = screen.getByText('Description, as stored').closest('details')!;
    expect(details).not.toHaveAttribute('open');
    expect(details.querySelector('pre')).toHaveTextContent('Plain words, {not json');
  });

  it('says when no description bytes were recorded', () => {
    render(<StoredDescription description={null} />);
    expect(screen.getByText('No description bytes recorded.')).toBeInTheDocument();
  });
});

describe('MnftClassEvidence', () => {
  const issuerId = '0x' + '8f'.repeat(20);
  const classDetail = {
    classId: issuerId + '00000014',
    issuerId,
    name: 'Santa',
    description: null,
    renderer: 'https://example.com/santa.png',
    total: 0,
    issued: 1200,
    configure: 0,
    permissions: {
      allowClaim: true,
      allowLock: true,
      allowExtension: true,
      allowCharacteristicUpdate: true,
      allowTransferBeforeClaim: true,
      allowTransferAfterClaim: true,
      allowDestroyBeforeClaim: true,
      allowDestroyAfterClaim: true,
    },
  };
  it('draws the issuer and class index and folds the issuer info as JSON', () => {
    const info = Array.from(new TextEncoder().encode('{"name":"Nervina","avatar":"x"}'), (b) =>
      b.toString(16).padStart(2, '0')
    ).join('');
    render(
      <MnftClassEvidence
        classDetail={classDetail}
        issuerDetail={{
          issuerId,
          name: 'Nervina',
          classCount: 24,
          setCount: 0,
          infoHex: `0x${info}`,
        }}
      />
    );
    expect(
      screen.getByRole('button', { name: /^Class index · big-endian = 20 · / })
    ).toBeInTheDocument();
    expect(screen.getByText('1,200 · planned 0')).toBeInTheDocument();
    expect(screen.getByText('Nervina · 24 classes · 0 sets')).toBeInTheDocument();
    const info2 = screen.getByText('Issuer info, as stored · pretty JSON').closest('details')!;
    expect(JSON.parse(info2.querySelector('pre')!.textContent!)).toEqual({
      name: 'Nervina',
      avatar: 'x',
    });
  });

  it('fails fast when the class id does not start with its issuer id', () => {
    const log = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      expect(() =>
        render(
          <MnftClassEvidence classDetail={{ ...classDetail, issuerId: '0x' + '11'.repeat(20) }} />
        )
      ).toThrow('issuer bytes 0x8f8f');
    } finally {
      log.mockRestore();
    }
  });
});
