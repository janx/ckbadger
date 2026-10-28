import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, fireEvent, within } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { latestActivities } from '@/__tests__/msw/fixtures/home';
import { DetailLayout, DetailSection } from '@/components/ui/detail-layout';
import { ListTable } from '@/components/ui/list-table';
import { ClassMark } from '@/components/ui/class-mark';
import { CapacityPartition } from '@/components/ui/capacity-partition';
import { CellBar } from '@/components/ui/cell-bar';
import { CellTile, type CellTileData } from '@/components/ui/cell-tile';
import { HexMap } from '@/components/ui/hex-map';
import { DnaStrip } from '@/components/ui/dna-strip';
import { AlleleGel } from '@/components/ui/allele-gel';
import { Chart } from '@/components/ui/chart';
import { TxStructure } from '@/components/transaction/tx-structure';
import { ActivityRow } from '@/components/activity/activity-row';
import { CrashPage } from '@/components/crash-page';
import { CELL_COLORS, CLASS_COLORS } from '@/lib/design-tokens';

vi.mock('@/components/error-vessels', () => ({
  ErrorVessels: () => <div data-testid="error-vessels" />,
  VesselsCaption: () => <span>Bounded live cell sample</span>,
}));

const cell: CellTileData = {
  id: 'in:0',
  href: '/cell/0xabc:0',
  label: 'input 0',
  owner: 'Alice',
  lockKind: 'JoyID',
  kind: 'tokens',
  content: 'USDI',
  capacity: '10000000000',
  occupied: '6100000000',
  state: 'consumed',
};

describe('detail and list foundations', () => {
  it('keeps domain content above a 72 px evidence boundary and raw data collapsed', () => {
    const { container } = render(
      <DetailLayout
        title="Transaction"
        identity="0xabc"
        facts={[{ label: 'Block', value: '12' }]}
        tier2={<p>Script evidence</p>}
        rawData={<code>0x0012</code>}
      >
        <DetailSection title="Activity">A transfer</DetailSection>
      </DetailLayout>
    );
    expect(screen.getByRole('heading', { name: 'Transaction' })).toHaveClass('text-[26px]');
    expect(container.querySelector('[data-tier="2"]')).toHaveClass('mt-[72px]');
    expect(container.querySelector('details')).not.toHaveAttribute('open');
    expect(container.querySelector('dt')).toHaveTextContent('Block');
    expect(container.querySelector('dd')).toHaveTextContent('12');
    expect(screen.getByRole('heading', { name: 'Activity' })).toHaveClass('text-[15px]');
  });
  it('sets index titles to 22 px and omits tier two on developer-only pages', () => {
    const { container } = render(
      <DetailLayout title="Blocks" kind="index">
        rows
      </DetailLayout>
    );
    expect(screen.getByRole('heading', { name: 'Blocks' })).toHaveClass('text-[22px]');
    expect(container.querySelector('[data-tier="2"]')).toBeNull();
  });
  it('budgets fixed columns, prints the entire exact value and provides compact labelled facts', () => {
    const amount = '900719925474099300.12345678';
    render(
      <ListTable
        label="Holdings"
        columns={[
          { key: 'name', label: 'Name', minWidth: 180, render: (r) => r.name },
          {
            key: 'amount',
            label: 'Capacity',
            minWidth: 240,
            numeric: true,
            render: (r) => r.amount,
          },
        ]}
        rows={[{ name: 'Token', amount }]}
        rowKey={(r) => r.name}
      />
    );
    const table = screen.getByRole('table', { name: 'Holdings' });
    expect(table).toHaveClass('table-fixed');
    expect(table).toHaveStyle({ minWidth: '420px' });
    expect(within(table).getByRole('cell', { name: amount })).toHaveClass(
      'text-right',
      'tabular-nums'
    );
    expect(screen.getByRole('list', { name: 'Holdings, compact' })).toHaveTextContent(amount);
  });
  it('uses canonical class marks without borrowing gold for token/object/identity', () => {
    render(
      <>
        {(['dao', 'tokens', 'objects', 'identities'] as const).map((kind) => (
          <ClassMark key={kind} kind={kind} />
        ))}
      </>
    );
    for (const kind of ['dao', 'tokens', 'objects', 'identities'] as const)
      expect(screen.getByRole('img', { name: `${kind} class` })).toHaveAttribute(
        'stroke',
        CLASS_COLORS[kind]
      );
  });
});

describe('exact capacity and cell anatomy', () => {
  it('draws the supplied partition with jade occupied, neutral free and DAO gold', () => {
    const { container } = render(
      <CapacityPartition
        subject="Alice"
        total="10000000000"
        occupied="6100000000"
        free="2900000000"
        daoFree="1000000000"
      />
    );
    expect(container.querySelector('[data-part="occupied"]')).toHaveStyle({
      width: '61%',
      backgroundColor: '#2edba3',
    });
    expect(container.querySelector('[data-part="DAO free"]')).toHaveStyle({
      width: '10%',
      backgroundColor: '#f2c55c',
    });
    expect(screen.getByText('61.00%')).toBeInTheDocument();
    expect(container.textContent).toContain('61.00000000 CKB');
  });
  it('fails with subject context instead of repairing a broken capacity partition', () => {
    expect(() =>
      render(<CapacityPartition subject="cell 0xabc:0" total="100" occupied="61" free="40" />)
    ).toThrow('cell 0xabc:0: capacity partition does not equal total 100');
  });
  it('accounts for the capacity field and all script/data bytes in canonical colours', () => {
    const { container } = render(
      <CellBar subject="0xabc:0" anatomy={{ lock: '53', type: '33', data: '16' }} />
    );
    expect(screen.getByRole('img')).toHaveAccessibleName('0xabc:0: 110 occupied bytes');
    for (const part of ['capacity', 'lock', 'type', 'data'] as const)
      expect(container.querySelector(`[data-part="${part}"]`)).toHaveStyle({
        backgroundColor: CELL_COLORS[part],
      });
    expect(container.querySelector('[data-part="capacity"]')).toHaveAttribute(
      'title',
      'capacity: 8 bytes'
    );
  });
  it('keeps exact capacity and the evidence link on cell tiles', () => {
    const { container } = render(<CellTile cell={cell} />);
    expect(screen.getByRole('link', { name: 'input 0' })).toHaveAttribute(
      'href',
      '/mainnet/cell/0xabc:0'
    );
    expect(container.textContent).toContain('100.00000000 CKB');
    expect(screen.getByText('JoyID')).toBeInTheDocument();
    expect(() => render(<CellTile cell={{ ...cell, occupied: '10000000001' }} />)).toThrow(
      'in:0: occupied'
    );
  });
});

describe('byte and DNA inspection', () => {
  const fields = [
    {
      id: 'flags',
      label: 'Flags',
      offset: 0,
      length: 1,
      description: 'Protocol version byte',
      color: CELL_COLORS.type,
    },
    {
      id: 'body',
      label: 'Body',
      offset: 1,
      length: 2,
      description: 'Payload bytes',
      color: CELL_COLORS.data,
    },
  ];
  it('maps bytes to source offsets and links field hover/focus to its exact explanation', () => {
    const onHover = vi.fn();
    const { container } = render(
      <HexMap hex="0x0102ff" fields={fields} subject="tx 0xabc" onFieldHover={onHover} />
    );
    fireEvent.focus(screen.getByRole('button', { name: 'Body bytes 1–2' }));
    expect(screen.getByText('Payload bytes')).toBeInTheDocument();
    expect(container.querySelector('[data-byte-offset="1"]')).toHaveAttribute(
      'data-highlighted',
      'true'
    );
    expect(container.querySelector('[data-byte-offset="0"]')).toHaveAttribute(
      'data-highlighted',
      'false'
    );
    expect(onHover).toHaveBeenCalledWith('body');
  });
  it('rejects malformed hex and field ranges outside the payload', () => {
    expect(() => render(<HexMap hex="0xabc" fields={[]} subject="cell X" />)).toThrow(
      'cell X: expected byte-aligned'
    );
    expect(() => render(<HexMap hex="0x00" fields={fields} subject="cell X" />)).toThrow(
      'field body range 1+2 exceeds 1 bytes'
    );
  });
  it('draws every real nibble in its lane in a DNA strip', () => {
    const { container } = render(<DnaStrip dna="0x0af1" />);
    const marks = container.querySelectorAll('[data-nibble]');
    expect(marks).toHaveLength(4);
    expect(marks[1]).toHaveAttribute('y', '20');
    expect(marks[2]).toHaveAttribute('y', '30');
    expect(screen.getByRole('img')).toHaveAccessibleName('Object DNA: 2 bytes, 0af1');
  });
  it('links a decoded trait to its gene bands without inventing a decode', () => {
    const onHover = vi.fn();
    const { container } = render(
      <AlleleGel
        dna="0x0102ff"
        subject="Object X"
        genes={fields.map((f) => ({ ...f, value: f.id === 'flags' ? 'v1' : 'raw payload' }))}
        onGeneHover={onHover}
      />
    );
    fireEvent.focus(container.querySelector('[data-gene="body"]')!);
    expect(onHover).toHaveBeenCalledWith('body');
    expect(container.querySelector('[data-nibble="0"]')).toHaveAttribute('opacity', '0.15');
    expect(container.querySelector('[data-nibble="2"]')).toHaveAttribute('opacity', '1');
    expect(screen.getByText('raw payload')).toBeInTheDocument();
  });
});

describe('transaction and activity foundations', () => {
  it('keeps a full dependency outpoint inside a shrinkable script-group column', () => {
    const outpoint = `0x${'ab'.repeat(32)}:4294967295 (depGroup)`;
    const { container } = render(
      <TxStructure
        txHash="0xlong-dependency"
        inputs={[cell]}
        outputs={[]}
        fee="0"
        groups={[
          {
            id: 'lock:long',
            kind: 'lock',
            label: 'long dependency',
            href: '/scripts/long',
            cellIds: [cell.id],
            executed: true,
            witness: 'Witness 0',
            dependency: outpoint,
          },
        ]}
      />
    );
    const group = container.querySelector('[data-group-id="lock:long"]');
    expect(group).toHaveClass('min-w-0');
    const dependency = screen.getByText(outpoint);
    expect(dependency).toHaveClass('min-w-0', 'break-all');
    expect(dependency).toHaveTextContent(outpoint);
  });
  it('cross-highlights resolved script groups, cells and continuity using hover and keyboard focus', () => {
    const output = { ...cell, id: 'out:0', label: 'output 0', state: 'live' };
    const { container } = render(
      <TxStructure
        txHash="0xtx"
        inputs={[cell]}
        outputs={[output]}
        fee="100"
        groups={[
          {
            id: 'lock:0',
            kind: 'lock',
            label: 'JoyID',
            href: '/scripts/JoyID',
            cellIds: ['in:0'],
            executed: true,
            witness: 'Witness 0',
            dependency: 'dep 0',
          },
        ]}
        continuity={[{ from: 'in:0', to: 'out:0', label: 'USDI' }]}
      />
    );
    fireEvent.focus(container.querySelector('[data-group-id="lock:0"]')!);
    expect(container.querySelector('[data-cell-id="in:0"]')).toHaveAttribute(
      'data-highlighted',
      'true'
    );
    expect(container.querySelector('[data-cell-id="out:0"]')).toHaveClass('opacity-30');
    fireEvent.mouseEnter(container.querySelector('[data-cell-id="in:0"]')!);
    expect(container.querySelector('[data-cell-id="out:0"]')).toHaveAttribute(
      'data-highlighted',
      'true'
    );
    expect(screen.getByText('Witness 0')).toBeInTheDocument();
  });
  it('rejects a dangling script membership with transaction context', () => {
    expect(() =>
      render(
        <TxStructure
          txHash="0xbroken"
          inputs={[cell]}
          outputs={[]}
          fee="0"
          groups={[
            {
              id: 'g',
              kind: 'lock',
              label: 'lock',
              href: '/scripts/lock',
              cellIds: ['missing'],
              executed: true,
              witness: null,
              dependency: null,
            },
          ]}
        />
      )
    ).toThrow('0xbroken: script group g references unknown cell missing');
  });
  it('keeps participant pool evidence nullable and never creates a block link', () => {
    const activity = latestActivities[0];
    const { container } = render(
      <ActivityRow
        activity={{
          ...activity,
          blockNumber: null,
          timestamp: null,
          poolStatus: 'proposed',
          timeAddedToPool: new Date().toISOString(),
          interpretation: {
            status: 'partial',
            reasons: [{ code: 'unresolved_input', detail: '0xabc:1' }],
          },
        }}
        mode="participant"
        participant={activity.participants[0]}
      />
    );
    expect(screen.getByText('Proposed')).toBeInTheDocument();
    expect(screen.getByText(/in pool for/)).toBeInTheDocument();
    expect(screen.getByText(/partial: input not yet resolvable/)).toBeInTheDocument();
    expect(container.querySelector('a[href*="/blocks/"]')).toBeNull();
  });
  it('can omit the partition total when the detail hero already displays it', () => {
    render(
      <CapacityPartition
        subject="Address"
        total="10000000000"
        occupied="6100000000"
        free="3900000000"
        showTotal={false}
      />
    );
    expect(screen.queryByText('CKB held')).not.toBeInTheDocument();
    expect(screen.getByText('occupied')).toBeInTheDocument();
    expect(screen.getByText('free')).toBeInTheDocument();
  });
  it('shares cross-user and participant activity grammar, including registered lock kind', () => {
    const activity = latestActivities[0];
    const participant = { ...activity.participants[0], lockKind: 'JoyID' };
    const { container } = render(
      <ActivityRow activity={activity} mode="participant" participant={participant} />
    );
    expect(container.querySelectorAll('[data-party]')).toHaveLength(1);
    expect(screen.getByText('JoyID')).toBeInTheDocument();
    expect(container.querySelector('[data-role="evidence"]')).toHaveTextContent('#20,819,142');
  });
  it('prints crash context and recovery actions without claiming a missing chain entity', () => {
    render(<CrashPage message="block 12: invalid DAO field" />);
    expect(screen.getByRole('alert')).toHaveTextContent('block 12: invalid DAO field');
    expect(screen.getByRole('button', { name: 'Try again →' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Back to the explorer →' })).toHaveAttribute(
      'href',
      '/mainnet'
    );
  });
});

describe('one chart grammar', () => {
  beforeEach(() => stubChartWidth(390));
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });
  const data = [
    { label: 'Oct 24', values: { rate: '2.5' } },
    { label: 'Oct 25', values: { rate: '3.0' } },
  ];
  it('uses physical units, nice ticks and keyboard exact readout with hardfork evidence', () => {
    const { container } = render(
      <Chart
        title="Hash rate"
        subtitle="Daily"
        data={data}
        series={[{ key: 'rate', label: 'Rate' }]}
        unit="PH/s"
        markers={[{ index: 0, label: 'Mirana', href: '/hardforks' }]}
      />
    );
    const chart = screen.getByRole('img', { name: 'Hash rate, PH/s' });
    expect(chart).toHaveAttribute('viewBox', '0 0 390 220');
    expect(chart).toHaveTextContent('2.4 PH/s');
    fireEvent.keyDown(chart, { key: 'ArrowLeft' });
    expect(container.querySelector('[aria-live]')).toHaveTextContent('Oct 24: Rate 2.5 PH/s');
    expect(screen.getByRole('link', { name: 'Mirana · Oct 24' })).toHaveAttribute(
      'href',
      '/mainnet/hardforks'
    );
  });
  it('retains exact large decimal readouts independently of geometry precision', () => {
    render(
      <Chart
        title="Capacity"
        subtitle="Daily"
        data={[{ label: 'Today', values: { v: '9007199254740993.12345678' } }]}
        series={[{ key: 'v', label: 'Capacity' }]}
        unit="CKB"
      />
    );
    expect(screen.getByText('9007199254740993.12345678')).toBeInTheDocument();
  });
  it('draws canonical class stacks and ranked composition without a rainbow', () => {
    const { container } = render(
      <Chart
        title="Classes"
        subtitle="Daily"
        data={[{ label: 'Today', values: { t: '12', o: '4' } }]}
        series={[
          { key: 't', label: 'Tokens', assetClass: 'tokens' },
          { key: 'o', label: 'Objects', assetClass: 'objects' },
        ]}
        kind="stacked"
        unit="CKB"
      />
    );
    expect(container.querySelector(`path[fill="${CLASS_COLORS.tokens}"]`)).not.toBeNull();
    expect(container.querySelector(`path[fill="${CLASS_COLORS.objects}"]`)).not.toBeNull();
  });
  it('supports a shared time axis and controlled hover without changing exact readouts', () => {
    const onActive = vi.fn();
    const { container } = render(
      <Chart
        title="Shared"
        subtitle="Daily"
        data={[
          { label: 'First', x: 100, values: { v: '10000000000000001' } },
          { label: 'Last', x: 300, values: { v: '2' } },
        ]}
        series={[{ key: 'v', label: 'Value' }]}
        unit="CKB"
        xDomain={[100, 300]}
        activeIndex={0}
        onActiveIndexChange={onActive}
        hideCaption
      />
    );
    expect(container.querySelector('[aria-live]')).toHaveTextContent(
      'First: Value 10000000000000001 CKB'
    );
    fireEvent.keyDown(screen.getByRole('img', { name: 'Shared, CKB' }), { key: 'ArrowRight' });
    expect(onActive).toHaveBeenCalledWith(1);
  });
  it('fails loudly for missing series instead of drawing a default zero', () => {
    expect(() =>
      render(
        <Chart
          title="Capacity"
          subtitle="Daily"
          data={[{ label: 'Oct 25', values: {} }]}
          series={[{ key: 'v', label: 'Capacity' }]}
          unit="CKB"
        />
      )
    ).toThrow('Capacity: missing v at Oct 25');
  });
});
