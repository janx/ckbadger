import { fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import listFixture from '@/__tests__/fixtures/scripts/class-evidence-list.json';
import detailFixture from '@/__tests__/fixtures/scripts/class-evidence-detail.json';
import multisigFixture from '@/__tests__/fixtures/scripts/multisig-evidence-detail.json';
import type {
  ScriptCodeClassTotalApi,
  ScriptHistoryMarkerApi,
  StackedAreaChartResponse,
} from '@/lib/api';
import {
  ScriptCodeClassSummary,
  ScriptCapacityEvidenceChart,
  ScriptCustodyEvidence,
  ScriptCodeHistory,
} from '@/components/scripts/script-code-evidence';

describe('script code evidence', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });
  it('sums exact shannons across all classes without dropping unknown custody', () => {
    render(
      <ScriptCodeClassSummary
        classes={[
          {
            class: 'fixed',
            lockCellsCount: 2,
            lockOwnedCapacitySum: '900719925474099300',
            referenceCount: 1,
          },
          {
            class: 'unsupported',
            lockCellsCount: 1,
            lockOwnedCapacitySum: '900719925474099300',
            referenceCount: 1,
          },
        ]}
      />
    );
    expect(screen.getByText('50.00% · 2 live cells · 1 references')).toBeInTheDocument();
    expect(screen.getByText('50.00% · 1 live cells · 1 references')).toBeInTheDocument();
    expect(screen.getByText('Custody unsupported')).toBeInTheDocument();
    expect(
      screen.getByRole('img', { name: 'Live lock capacity distribution by code class' }).children
    ).toHaveLength(2);
  });

  it('consumes API-produced full-scope list totals and canonical history markers', () => {
    expect(listFixture.roleCounts).toEqual({ all: 1, locks: 1, types: 0 });
    expect(
      listFixture.classSummary.reduce(
        (sum, row) => sum + BigInt(row.lockOwnedCapacitySum),
        BigInt(0)
      )
    ).toBe(BigInt(900));
    render(
      <ScriptCodeClassSummary classes={listFixture.classSummary as ScriptCodeClassTotalApi[]} />
    );
    expect(screen.getByText(/55.55% · 1 live cells/)).toBeInTheDocument();
    expect(screen.getByText(/44.44% · 1 live cells/)).toBeInTheDocument();
    expect(detailFixture.history.every((marker) => marker.indexedDay === '2024-01-02')).toBe(true);
  });

  it('separates a code cell holder from a later verified spend', () => {
    render(
      <ScriptCustodyEvidence
        custody={{
          class: 'multisig',
          lockCodeHash: '0x01',
          lockHashType: 'type',
          lockArgs: '0x02',
          threshold: 3,
          keyCount: 5,
          requireFirstN: 0,
          keyHashes: [],
          verifiedByTx: `0x${'12'.repeat(32)}`,
          witnessState: 'verified',
          owners: [],
        }}
      />
    );
    expect(screen.getByText(/3 of 5 keys/)).toBeInTheDocument();
    expect(screen.getByText(/This holder signed to spend/)).toBeInTheDocument();
  });

  it('shows no observed events without inventing a deployment', () => {
    render(<ScriptCodeHistory markers={[]} />);
    expect(
      screen.getByText('No observed code cell events for registered references.')
    ).toBeInTheDocument();
  });

  it('positions an event on the indexed calendar day rather than a rolled-back raw timestamp', () => {
    render(
      <ScriptCodeHistory
        markers={[
          {
            referenceHash: `0x${'12'.repeat(32)}`,
            hashType: 'type',
            versionHash: `0x${'34'.repeat(32)}`,
            event: 'deployed',
            blockNumber: 42,
            txIndex: 1,
            timestamp: Date.parse('2024-01-01T23:59:59Z'),
            chainTimeMs: Date.parse('2024-01-02T00:00:01Z'),
            indexedDay: '2024-01-02',
            codeCellTxHash: `0x${'56'.repeat(32)}`,
            codeCellOutputIndex: 0,
            eventTxHash: `0x${'56'.repeat(32)}`,
            custody: {
              class: 'one_key',
              lockCodeHash: null,
              lockHashType: null,
              lockArgs: null,
              threshold: null,
              keyCount: null,
              requireFirstN: null,
              keyHashes: [],
              verifiedByTx: null,
              witnessState: 'unavailable',
              owners: [],
            },
          },
        ]}
      />
    );
    expect(screen.getByText(/Chart day 2024-01-02/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Event transaction/ })).toHaveAttribute(
      'href',
      `/mainnet/tx/0x${'56'.repeat(32)}`
    );
  });

  it('aligns API-produced upgrade markers with the indexed chart day and keeps never-signed distinct', () => {
    const markers = multisigFixture.history as ScriptHistoryMarkerApi[];
    const chart: StackedAreaChartResponse = {
      title: 'Capacity',
      series: [{ key: 'occupied', label: 'Occupied', color: '#fff' }],
      data: [
        { date: '2024-01-01', values: { occupied: '100000000' } },
        { date: '2024-01-02', values: { occupied: '300000000' } },
      ],
    };
    render(<ScriptCapacityEvidenceChart chart={chart} markers={markers} />);
    expect(
      screen.getAllByRole('link', { name: /code · 3 events · 2024-01-02/ }).length
    ).toBeGreaterThan(0);
    fireEvent.click(screen.getAllByRole('button', { name: /2024-01-02 · deployed · #10/ })[0]);
    expect(screen.getByText(/Never signed: every indexed cell/)).toBeInTheDocument();
    expect(markers.some((marker) => marker.custody.witnessState === 'unavailable')).toBe(true);
    expect(
      screen.getByLabelText('Custodian band aligned with capacity chart days')
    ).toBeInTheDocument();
  });

  it('keeps nearby plot marker labels compact while retaining complete event links', () => {
    stubChartWidth(390);
    const seed = multisigFixture.history[0] as ScriptHistoryMarkerApi;
    const markers = [
      { ...seed, indexedDay: '2024-08-01', blockNumber: 8 },
      { ...seed, indexedDay: '2024-11-01', blockNumber: 11, codeCellOutputIndex: 1 },
    ];
    const chart: StackedAreaChartResponse = {
      title: 'Capacity',
      series: [{ key: 'used', label: 'Used', color: '#fff' }],
      data: [
        { date: '2024-08-01', values: { used: '100000000' } },
        { date: '2024-11-01', values: { used: '200000000' } },
      ],
    };
    const { container } = render(<ScriptCapacityEvidenceChart chart={chart} markers={markers} />);
    expect(
      [...container.querySelectorAll('svg text')].filter((node) => node.textContent === '◆')
    ).toHaveLength(2);
    expect(screen.getAllByRole('link', { name: /code · 1 event/ })).toHaveLength(2);
    expect(screen.getAllByRole('button', { name: /2024-08-01 · deployed/ })).toHaveLength(1);
    expect(screen.getAllByRole('button', { name: /2024-11-01 · deployed/ })).toHaveLength(1);
  });

  it('pairs upgrade custody only within the same exact reference when a transaction upgrades two anchors', () => {
    const seed = multisigFixture.history[0] as ScriptHistoryMarkerApi;
    const verified = (keyHashes: string[]) => ({
      ...seed.custody,
      witnessState: 'verified' as const,
      keyHashes,
      threshold: 1,
      keyCount: keyHashes.length,
      verifiedByTx: '0x' + '77'.repeat(32),
    });
    const tx = '0x' + '77'.repeat(32);
    const refA = '0x' + 'aa'.repeat(32);
    const refB = '0x' + 'bb'.repeat(32);
    const markers: ScriptHistoryMarkerApi[] = [
      {
        ...seed,
        referenceHash: refA,
        event: 'consumed',
        eventTxHash: tx,
        custody: verified(['a', 'b']),
      },
      {
        ...seed,
        referenceHash: refB,
        event: 'consumed',
        eventTxHash: tx,
        custody: verified(['x', 'y']),
      },
      {
        ...seed,
        referenceHash: refA,
        event: 'deployed',
        eventTxHash: tx,
        codeCellOutputIndex: 1,
        custody: verified(['a', 'c']),
      },
      {
        ...seed,
        referenceHash: refB,
        event: 'deployed',
        eventTxHash: tx,
        codeCellOutputIndex: 2,
        custody: verified(['x', 'z']),
      },
    ];
    const chart: StackedAreaChartResponse = {
      title: 'Capacity',
      series: [{ key: 'occupied', label: 'Occupied', color: '#fff' }],
      data: [{ date: seed.indexedDay, values: { occupied: '100000000' } }],
    };
    render(<ScriptCapacityEvidenceChart chart={chart} markers={markers} />);
    expect(screen.getByText(/Retained 1 of 2 previous keys; replaced 1/)).toBeInTheDocument();
    fireEvent.click(screen.getAllByRole('button', { name: /deployed/ })[0]);
    expect(screen.getByText(/Retained 1 of 2 previous keys; replaced 1/)).toBeInTheDocument();
  });

  it('labels capacity as occupied and free without duplicating CKB units', () => {
    render(
      <ScriptCapacityEvidenceChart
        markers={[]}
        chart={{
          title: 'Capacity',
          series: [
            { key: 'used', label: 'Used', color: '#f59e0b' },
            { key: 'unused', label: 'Unused', color: '#00c389' },
          ],
          data: [{ date: '2024-01-02', values: { used: '100000000', unused: '200000000' } }],
        }}
      />
    );
    expect(screen.getByText('Occupied')).toBeInTheDocument();
    expect(screen.getByText('Free')).toBeInTheDocument();
    expect(document.body.textContent).not.toContain('CKB CKB');
    expect(document.body.textContent).toContain('1.00000000 CKB');
  });
});
