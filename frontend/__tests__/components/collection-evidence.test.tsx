import { JOYID_CODE_HASH, seededAddress } from '@/__tests__/utils/ckb-address';
import { cleanup, fireEvent, render, screen, within } from '@/__tests__/utils/test-utils';
import {
  ClusterCellEvidence,
  MnftClassEvidence,
  StoredDescription,
} from '@/components/asset/collection-evidence';
import { clusterProtocolProducer } from '@/__tests__/msw/fixtures/item-producer';
import type { ItemCellVersion } from '@/lib/api';
import { fullText } from '@/__tests__/utils/full-text';

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
      // U-09 (re-pinned): the evidence's rows at the row size; R9 (re-pinned): `ckb1…` and 8
      // B4 fix round (re-pinned): the producer's owner is a real address now, its tail read off it;
      // second fix round (re-pinned): its lock by the one decision, 'lock 0x…' (it was
      // 'Unregistered lock')
      `live cell0xa5c53484…1f3261:0 · created in #99 · 2023-11-15held byckb1…${current.cell.ownerAddress.slice(-8)} · lock 0x41414141…414141type0x7366a615…030075 · the cluster id is this cell’s type args`
    );
    expect(within(facts).getAllByRole('link')[0]).toHaveAttribute(
      'href',
      `/mainnet/cell/${current.cell.txHash}-0`
    );
  });
});

// B4 second fix round (review-rules-fixes-logic SHOULD 3, tests SHOULD-3; the coordinator's
// item 5): the holder's lock beside its address reads the one decision, off the cell's lock:
// nothing for the default lock, "lock 0x…" for a lock the registry does not name (it was
// "Unregistered lock"), its hash type after it for a lock on the default code hash by data.
describe('ClusterCellEvidence names the holder’s lock by the one decision', () => {
  const DEFAULT = '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8';
  const heldBy = (cell: Partial<ItemCellVersion>) => {
    const { current, clusterId } = clusterProtocolProducer;
    render(
      <ClusterCellEvidence
        clusterId={clusterId}
        current={{ ...current, cell: { ...current.cell, ...cell } }}
      />
    );
    const facts = screen.getByLabelText('The cluster cell');
    return within(facts).getByText('held by').nextElementSibling as HTMLElement;
  };

  it('writes an unregistered lock by its code hash', () => {
    const { lock, ownerAddress } = clusterProtocolProducer.current.cell;
    expect(heldBy({})).toHaveTextContent(
      new RegExp(`^ckb1…${ownerAddress.slice(-8)} · lock 0x41414141…414141$`)
    );
    expect(lock.codeHash).toBe(`0x${'41'.repeat(32)}`);
  });

  it('writes nothing for the default lock, and a by-data lock with its hash type', () => {
    const args = '0xb39bbc0b3673c7d36450bc14cfcdad2d559c6c64';
    expect(
      heldBy({
        ownerAddress:
          'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqdnnw7qkdnnclfkg59uzn8umtfd2kwxceqxwquc4',
        lock: { codeHash: DEFAULT, hashType: 'type', args },
        lockName: 'SECP256K1/blake160',
      })
    ).toHaveTextContent(/^ckb1…eqxwquc4$/);
    cleanup();
    expect(
      heldBy({
        ownerAddress:
          'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq9nnw7qkdnnclfkg59uzn8umtfd2kwxceqvguktl',
        lock: { codeHash: DEFAULT, hashType: 'data', args },
        lockName: 'SECP256K1/blake160',
      })
    ).toHaveTextContent(/^ckb1…eqvguktl · lock 0x9bd7e06f…a3cce8 by data$/);
  });
});

// Lane B4 third round (review-rules-fixes2 SHOULD 1, the coordinator's item 3, ruling D7): the
// lock decision's refusals placed above their parts fail the part they are in, said in its place,
// never the page: here a cell whose owner address spells another lock (JoyID) than its own.
describe('ClusterCellEvidence fails its holder’s lock in place', () => {
  it('says the lock cannot be read where it stands, and draws the rest of the cell', () => {
    const { current, clusterId } = clusterProtocolProducer;
    const joy = seededAddress('joycl', JOYID_CODE_HASH);
    render(
      <ClusterCellEvidence
        clusterId={clusterId}
        current={{ ...current, cell: { ...current.cell, ownerAddress: joy } }}
      />
    );
    const facts = screen.getByLabelText('The cluster cell');
    const heldBy = within(facts).getByText('held by').nextElementSibling as HTMLElement;
    expect(within(heldBy).getByRole('alert')).toHaveTextContent(
      new RegExp(`address ${joy} spells the lock`)
    );
    expect(facts).toHaveTextContent('the cluster id is this cell’s type args');
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
    // each count is one unit (B3 round 4, task 3): the line is read across their spans
    expect(screen.getByText(fullText('Nervina · 24 classes · 0 sets'))).toBeInTheDocument();
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
