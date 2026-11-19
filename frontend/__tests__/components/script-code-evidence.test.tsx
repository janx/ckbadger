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
  ScriptCodeClassBar,
  ScriptCodeClassSummary,
  ScriptCapacityEvidenceChart,
  ScriptCustodyEvidence,
} from '@/components/scripts/script-code-evidence';
import { fullText } from '@/__tests__/utils/full-text';

describe('script code evidence', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });
  it('sums exact shannons across all classes without dropping unknown custody', () => {
    const classes: ScriptCodeClassTotalApi[] = [
      {
        class: 'fixed',
        lockCellsCount: 2,
        lockOwnedCapacitySum: '900719925474099300',
        referenceCount: 1,
        familyCount: 1,
      },
      {
        class: 'unsupported',
        lockCellsCount: 1,
        lockOwnedCapacitySum: '900719925474099300',
        referenceCount: 1,
        familyCount: 1,
      },
    ];
    // Hierarchy v3: the bar is the index hero's (ScriptCodeClassBar); the summary is tier 2's.
    render(
      <>
        <ScriptCodeClassSummary lockScriptCount={2} classes={classes} />
        <ScriptCodeClassBar classes={classes} />
      </>
    );
    const sentence = screen.getByTestId('code-class-sentence').textContent;
    // re-pinned on purpose (lane P2's follow-up, item 5): a sentence writes the summary form, whole CKB from
    // 1,000 up, the exact value its title (it wrote the column's every decimal, padded)
    expect(sentence).toContain('Of the 18,014,398,509 CKB under these');
    expect(screen.getByTestId('code-class-sentence').querySelector('[title]')).toHaveAttribute(
      'title',
      expect.stringContaining('18,014,398,509.481986')
    );
    expect(sentence).toContain('50.00% runs code no one can change');
    expect(sentence).toContain('50.00% sits under code marked "holder lock not recognised"');
    expect(screen.getByText('Holder lock not recognised')).toBeInTheDocument();
    expect(
      screen.getByRole('img', { name: 'CKB under lock scripts by who can change their code' })
        .children
    ).toHaveLength(2);
  });

  it('consumes API-produced full-scope list totals and canonical history markers', () => {
    // re-pinned on purpose (the P3 follow-up, SHOULD 3): the route states the overlap and the rest
    expect(listFixture.roleCounts).toEqual({ all: 1, locks: 1, types: 0, both: 0, neither: 0 });
    expect(
      listFixture.classSummary.reduce(
        (sum, row) => sum + BigInt(row.lockOwnedCapacitySum),
        BigInt(0)
      )
    ).toBe(BigInt(900));
    render(
      <ScriptCodeClassSummary
        classes={listFixture.classSummary as ScriptCodeClassTotalApi[]}
        lockScriptCount={listFixture.roleCounts.locks}
      />
    );
    const sentence = screen.getByTestId('code-class-sentence').textContent;
    // re-pinned for lane exactness, part B2 (one percentage, cut): 5 of 9 shannons is 55.555…%, 55.55% (half up 55.56%)
    expect(sentence).toContain('55.55% runs code no one can change');
    expect(sentence).toContain('44.44% runs code a single key can replace');
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
          verifiedCodeCell: `0x${'34'.repeat(32)}:1`,
          witnessState: 'verified',
          owners: [],
        }}
      />
    );
    expect(screen.getByText(fullText(/3 of 5 keys/))).toBeInTheDocument();
    expect(screen.getByText(/This holder signed to spend/)).toBeInTheDocument();
    // Lane P2's third round (item 16): the spending transaction opens at the code cell it spent
    // (`transactionAtCellHref`), which its page then never folds away (it linked the transaction
    // bare)
    expect(screen.getByRole('link', { name: /0x12121212/ })).toHaveAttribute(
      'href',
      `/mainnet/tx/0x${'12'.repeat(32)}#0x${'34'.repeat(32)}-1`
    );
  });

  it('links every code cell each holder owns to its cell page', () => {
    const txA = `0x${'aa'.repeat(32)}`;
    const txB = `0x${'bb'.repeat(32)}`;
    const owner = (lockArgs: string, codeCells: string[]) => ({
      lockCodeHash: '0x01',
      lockHashType: 'type',
      lockArgs,
      class: 'one_key' as const,
      codeCells,
    });
    render(
      <ScriptCustodyEvidence
        custody={{
          class: 'mixed',
          lockCodeHash: null,
          lockHashType: null,
          lockArgs: null,
          threshold: null,
          keyCount: null,
          requireFirstN: null,
          keyHashes: [],
          verifiedByTx: null,
          verifiedCodeCell: null,
          witnessState: 'not_applicable',
          owners: [owner('0x02', [`${txA}:0`, `${txA}:1`]), owner('0x03', [`${txB}:2`])],
        }}
      />
    );
    expect(screen.getByRole('link', { name: `${txA}:0` })).toHaveAttribute(
      'href',
      `/mainnet/cell/${txA}-0`
    );
    expect(screen.getByRole('link', { name: `${txA}:1` })).toHaveAttribute(
      'href',
      `/mainnet/cell/${txA}-1`
    );
    expect(screen.getByRole('link', { name: `${txB}:2` })).toHaveAttribute(
      'href',
      `/mainnet/cell/${txB}-2`
    );
    expect(screen.getByText(fullText(/2 code cells:/))).toBeInTheDocument();
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
      series: [{ key: 'occupiedCapacity', label: 'Occupied', color: '#fff' }],
      data: [
        { date: '2024-08-01', values: { occupiedCapacity: '100000000' } },
        { date: '2024-11-01', values: { occupiedCapacity: '200000000' } },
      ],
    };
    const { container } = render(<ScriptCapacityEvidenceChart chart={chart} markers={markers} />);
    expect(
      [...container.querySelectorAll('svg text')].filter((node) => node.textContent === '◆')
    ).toHaveLength(2);
    expect(screen.getAllByRole('link', { name: /code · 1 event/ })).toHaveLength(2);
    expect(screen.getAllByRole('button', { name: /2024-08-01 · deployed/ })).toHaveLength(1);
    expect(screen.getAllByRole('button', { name: /2024-11-01 · deployed/ })).toHaveLength(1);
    // Each chip names its code cell by out point (not the reference hash), opening the cell page.
    const codeCell = seed.codeCellTxHash;
    // U-09 (re-pinned): the marker rows' out points at the row size
    const short = `${codeCell.slice(0, 10)}…${codeCell.slice(-6)}`;
    expect(screen.getByRole('link', { name: `${short}:0` })).toHaveAttribute(
      'href',
      `/mainnet/cell/${codeCell}-0`
    );
    expect(screen.getByRole('link', { name: `${short}:1` })).toHaveAttribute(
      'href',
      `/mainnet/cell/${codeCell}-1`
    );
    expect(document.body.textContent).not.toContain(seed.referenceHash.slice(0, 8));
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
      verifiedCodeCell: `0x${'77'.repeat(32)}:0`,
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
            { key: 'occupiedCapacity', label: 'Occupied', color: '#f59e0b' },
            { key: 'freeCapacity', label: 'Free', color: '#00c389' },
          ],
          data: [
            {
              date: '2024-01-02',
              values: {
                occupiedCapacity: '100000000',
                freeCapacity: '200000000',
                capacity: '300000000',
              },
            },
          ],
        }}
      />
    );
    expect(screen.getByText('Occupied')).toBeInTheDocument();
    expect(screen.getByText('Free')).toBeInTheDocument();
    expect(document.body.textContent).not.toContain('CKB CKB');
    expect(document.body.textContent).toContain('1.00000000 CKB');
  });
});
