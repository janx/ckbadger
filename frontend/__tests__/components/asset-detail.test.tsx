import { render, screen, fireEvent } from '@/__tests__/utils/test-utils';
import { AssetDetailShell, AssetMetrics, AssetReadState } from '@/components/asset/detail-shell';
import { ledeRestatements } from '@/__tests__/utils/lede-rule';
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
    expect(screen.getByRole('heading', { level: 1, name: 'Object' })).toHaveClass('text-entity');
    expect(container.querySelector('[data-tier="1"]')).toHaveTextContent('Capacity history');
    expect(container.querySelector('[data-tier="2"]')).toHaveClass(
      'mt-24',
      'border-t',
      'border-base-border'
    );
    expect(screen.getByRole('navigation', { name: 'Breadcrumb' })).toHaveTextContent('Inventory');
  });
  it('wraps title, status, identity and a given lede in one hero section; the intro follows it (hierarchy v3, R13)', () => {
    render(
      <AssetDetailShell
        title="Nervape 3x"
        kind="objects"
        breadcrumb={[{ label: 'Inventory', href: '/inventory/objects' }]}
        status={<span>Live</span>}
        identity={<span>0x51c7</span>}
        lede={<p>Fully on-chain.</p>}
        intro={<dl aria-label="Traits" />}
      >
        <p>Capacity history</p>
      </AssetDetailShell>
    );
    const hero = screen.getByRole('region', { name: 'Nervape 3x' });
    expect(hero.tagName).toBe('SECTION');
    for (const text of ['Live', '0x51c7', 'Fully on-chain.']) expect(hero).toHaveTextContent(text);
    expect(hero).toContainElement(screen.getByRole('heading', { level: 1, name: 'Nervape 3x' }));
    const traits = screen.getByLabelText('Traits');
    expect(hero.contains(traits)).toBe(false);
    expect(hero.compareDocumentPosition(traits) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(screen.getByRole('banner')).toContainElement(traits);
  });
  it('keeps the title block without a hero section when no lede is given (old default)', () => {
    const { container } = render(
      <AssetDetailShell title="Token" kind="tokens" breadcrumb={[]} intro={<p>Intro</p>}>
        <p>Body</p>
      </AssetDetailShell>
    );
    expect(container.querySelector('section')).toBeNull();
    expect(screen.getByRole('banner')).toHaveTextContent('Intro');
  });
  // Consistency round C7: the rail's facts become one metrics row under the hero, and the page is
  // one column (no 260 px rail track, so tier 1 and the feed share one right edge).
  it('puts the metrics row under the hero in one column, without a facts rail (C7)', () => {
    const { container } = render(
      <AssetDetailShell
        title="Nervape Headwear"
        kind="objects"
        breadcrumb={[{ label: 'Inventory', href: '/inventory/objects' }]}
        lede={<p>Spore cluster · DOB/0.</p>}
        hero={<p>1,298 objects live</p>}
        metrics={
          <AssetMetrics
            items={[
              { label: 'Holders', value: '893', note: '748 hold one' },
              { label: 'Cluster cell held by', value: 'ckb1qzda…wgcjk9lz', size: 'body' },
            ]}
          />
        }
        feed={<p>Activities</p>}
      >
        <p>Objects</p>
      </AssetDetailShell>
    );
    const hero = screen.getByRole('region', { name: 'Nervape Headwear' });
    expect(hero).toHaveTextContent('1,298 objects live');
    const lede = hero.querySelector('[data-role="lede"]')!;
    expect(lede).toHaveTextContent('Spore cluster · DOB/0.');
    expect(lede).not.toHaveTextContent('1,298');
    const metrics = screen.getByRole('region', { name: 'Metrics' });
    expect(metrics).toHaveAttribute('data-role', 'metrics');
    const follows = (a: Node, b: Node) =>
      Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);
    expect(follows(screen.getByRole('banner'), metrics)).toBe(true);
    expect(follows(metrics, screen.getByText('Objects'))).toBe(true);
    // R13: meta label, section value, inline dim note; an address value stays at body size.
    const [holders, owner] = [...metrics.querySelectorAll('dl > div')];
    expect(holders.querySelector('dt')).toHaveClass('text-meta', 'text-text-dim');
    expect(holders.querySelector('dd')).toHaveClass('text-section', 'text-text-bright');
    expect(holders.querySelector('dd')).toHaveTextContent('893 · 748 hold one');
    expect(holders.querySelector('[data-role="metric-note"]')).toHaveClass(
      'text-small',
      'text-text-dim'
    );
    expect(owner.querySelector('dd')).toHaveClass('text-body');
    // One row from lg up: every metric in its own column of one grid row.
    expect(metrics.querySelector('dl')).toHaveClass('lg:grid-flow-col');
    expect(screen.queryByRole('complementary', { name: 'Facts' })).toBeNull();
    const grid = container.querySelector('[data-tier="1"]')!;
    expect(grid.className).not.toMatch(/grid-cols-\[minmax\(0,1fr\)_260px\]/);
  });
  it('keeps the facts rail and its track for a page that still passes facts (old default)', () => {
    const { container } = render(
      <AssetDetailShell
        title="Object"
        kind="objects"
        breadcrumb={[]}
        facts={[{ label: 'Owner', value: 'Alice' }]}
      >
        <p>Body</p>
      </AssetDetailShell>
    );
    expect(screen.getByRole('complementary', { name: 'Facts' })).toHaveTextContent('Alice');
    expect(container.querySelector('[data-tier="1"]')).toHaveClass(
      'lg:grid-cols-[minmax(0,1fr)_260px]'
    );
  });
  it('finds a metric restated in the lede (the C7 lede rule the page tests apply)', () => {
    render(
      <AssetDetailShell
        title="RGB++"
        kind="tokens"
        breadcrumb={[]}
        intro={<p>24,255,256.49 RGB++ exist, held by 11,866 addresses.</p>}
        metrics={<AssetMetrics items={[{ label: 'Holders', value: '11,866' }]} />}
      >
        <p>Body</p>
      </AssetDetailShell>
    );
    expect(ledeRestatements(document.body)).toEqual(['11,866']);
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
      `/mainnet/cell/${second.txHash}-4`
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
