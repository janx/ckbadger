import { render, screen, fireEvent } from '@/__tests__/utils/test-utils';
import { AssetDetailShell, AssetReadState } from '@/components/asset/detail-shell';
import { CellVersions } from '@/components/asset/cell-versions';
import type { ItemCellVersion } from '@/lib/api';
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));
const version: ItemCellVersion = {
  txHash: '0x' + '12'.repeat(32),
  outputIndex: 4,
  blockNumber: 10,
  txIndex: 1,
  timestamp: '1700000000000',
  isLive: true,
  consumedByTx: null,
  consumedAtBlock: null,
  ownerLockHash: '0x' + '34'.repeat(32),
  ownerAddress: 'ckb1owner',
  capacity: '10000000001',
  occupiedCapacity: '6100000000',
  lock: { codeHash: '0x' + '56'.repeat(32), hashType: 'data1', args: '0x' + '78'.repeat(20) },
  type: null,
  anatomy: { capacityBytes: 8, lockBytes: 53, typeBytes: 0, dataBytes: 0 },
  dataHash: '0x' + 'ab'.repeat(32),
  lockName: 'JoyID',
  typeName: null,
};
describe('Asset domain layout', () => {
  it('keeps history before feeds and raw evidence below the shared tier boundary', () => {
    const { container } = render(
      <AssetDetailShell
        title="Object"
        kind="objects"
        breadcrumb={[{ label: 'Inventory', href: '/inventory/objects' }]}
        facts={[{ label: 'Owner', value: 'Alice' }]}
        feed={<p>Activities</p>}
        tier2={<p>Raw cell data</p>}
      >
        <p>Capacity history</p>
      </AssetDetailShell>
    );
    expect(screen.getByRole('heading', { level: 1, name: 'Object' })).toHaveClass('text-[26px]');
    expect(container.querySelector('[data-tier="1"]')).toHaveTextContent('Capacity history');
    expect(container.querySelector('[data-tier="2"]')).toHaveClass('mt-[72px]');
    expect(screen.getByRole('navigation', { name: 'Breadcrumb' })).toHaveTextContent('Inventory');
  });
  it('retains an explicit error when previously loaded data remains', () => {
    render(<AssetReadState error={new Error('read failed')} label="history" retained />);
    expect(screen.getByRole('alert')).toHaveTextContent(
      'Could not refresh history: read failed. Showing previously loaded data.'
    );
  });
});
describe('Canonical item cell versions', () => {
  it('preserves single-shannon capacity, the real outpoint, hash type and focus association', () => {
    const second = { ...version, txHash: '0x' + '90'.repeat(32), blockNumber: 11 };
    render(
      <CellVersions
        subject="Object"
        history={{
          data: [
            { ...version, isLive: false, consumedByTx: second.txHash, consumedAtBlock: 11 },
            second,
          ],
          total: 2,
        }}
      />
    );
    expect(screen.getByRole('heading', { name: 'Cell versions' })).toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: 'CKB held in its cell' })).not.toBeInTheDocument();
    const versions = screen.getByLabelText('Object cell versions');
    expect(versions).toHaveTextContent('100.00000001');
    expect(versions).toHaveTextContent('39.00000001');
    expect(versions).toHaveTextContent('JoyID');
    expect(screen.getByRole('link', { name: 'Version 2 · current' })).toHaveAttribute(
      'href',
      `/mainnet/cell/${second.txHash}/4`
    );
    expect(versions.querySelector('a[href*="hashType=data1&kind=lock"]')).not.toBeNull();
    fireEvent.focus(versions.querySelectorAll('article')[1]);
    expect(versions.querySelectorAll('article')[0]).toHaveClass('opacity-35');
  });
  it('rejects missing versions and an inconsistent occupied byte count', () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() =>
      render(<CellVersions subject="Object" history={{ data: [version], total: 2 }} />)
    ).toThrow('incomplete cell-version history');
    expect(() =>
      render(
        <CellVersions
          subject="Object"
          history={{ data: [{ ...version, occupiedCapacity: '6200000000' }], total: 1 }}
        />
      )
    ).toThrow('anatomy does not equal occupied capacity');
    error.mockRestore();
  });
});
