import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import { stubLayoutWidths } from '@/__tests__/utils/phone-row';
import ScriptByCodeHashPage from '@/app/script/[codeHash]/client-page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    lookupScripts: vi.fn(),
    getCodeCell: vi.fn(),
    getCodeCells: vi.fn(),
    getCell: vi.fn(),
    getScriptCapacityChartByCodeHash: vi.fn(),
    getCellsByScriptRef: vi.fn(),
  },
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

const mockCodeHash = '0x7366a61534fa7c7e6225ecc0d828ea3b5366adec2b58206f2ee84995fe030075';
const mockDataHash = '0x709f3fda12f561cfacf92273c57a98fede188a3f1a59b1f888d113f9cce08649';
const mockGovernanceCodeHash = '0xf222222222222222222222222222222222222222222222222222222222222222';
const mockGovernanceArgs = '0xa222222222222222222222222222222222222222';
const mockDeploymentTxHash = '0x4444444444444444444444444444444444444444444444444444444444444444';
const { replaceMock } = vi.hoisted(() => ({ replaceMock: vi.fn() }));
let currentCodeHashParam = mockCodeHash;

vi.mock('@/src/navigation', () => ({
  useParams: () => ({ codeHash: currentCodeHashParam }),
  useSearchParams: () => new URLSearchParams(),
  useRouter: () => ({ replace: replaceMock }),
}));

const emptyCells = {
  data: [],
  total: 0,
  limit: 50,
  hasMore: false,
  nextCursor: null,
};

describe('ScriptByCodeHashPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    currentCodeHashParam = mockCodeHash;
    vi.mocked(api.lookupScripts).mockImplementation(async (codeHashes) => {
      const response: Record<string, any> = {};

      if (codeHashes.includes(mockCodeHash)) {
        response[mockCodeHash] = {
          referenceHash: mockCodeHash,
          codeHash: mockCodeHash,
          name: 'Unknown',
          scriptKind: 'type',
          decoderType: null,
          hashType: 'type',
          deploymentTypeHash: mockCodeHash,
          deploymentDataHash: mockDataHash,
          codeCellTxHash: mockDeploymentTxHash,
          codeCellOutputIndex: 0,
          liveCellsCount: 15,
          ownedCapacitySum: '25000000000',
          ownedKnowledgeSum: '14000000000',
          codeCellsLiveCount: 1,
          codeCellsTotal: 1,
          resolutionState: 'resolved',
          ambiguity: null,
        };
      }

      if (codeHashes.includes(mockGovernanceCodeHash)) {
        response[mockGovernanceCodeHash] = {
          referenceHash: mockGovernanceCodeHash,
          codeHash: mockGovernanceCodeHash,
          name: 'Governance Lock',
          scriptKind: 'lock',
          decoderType: null,
          hashType: 'type',
          deploymentTypeHash: mockGovernanceCodeHash,
          deploymentDataHash: null,
          codeCellTxHash: null,
          codeCellOutputIndex: null,
          liveCellsCount: 0,
          ownedCapacitySum: '0',
          ownedKnowledgeSum: '0',
          codeCellsLiveCount: 0,
          codeCellsTotal: 0,
          resolutionState: 'resolved',
          ambiguity: null,
        };
      }

      return response;
    });
    vi.mocked(api.getCodeCell).mockResolvedValue({ txHash: null, outputIndex: null });
    vi.mocked(api.getCodeCells).mockResolvedValue({
      codeCells: [
        {
          txHash: mockDeploymentTxHash,
          outputIndex: 0,
          status: 'live',
          createdAtBlock: 123456,
          capacity: '16100000000',
        },
      ],
      liveCount: 1,
      totalCount: 1,
      resolvedVersionHash: mockCodeHash,
      ambiguity: null,
    });
    vi.mocked(api.getCell).mockResolvedValue({
      txHash: mockDeploymentTxHash,
      outputIndex: 0,
      capacity: '16100000000',
      commonKnowledgeSize: '6100000000',
      lockScriptHash: '0xlock1',
      dataSize: 0,
      createdAtBlock: 123456,
      status: 'live',
      lock: {
        codeHash: mockGovernanceCodeHash,
        hashType: 'type',
        args: mockGovernanceArgs,
      },
    });
    vi.mocked(api.getScriptCapacityChartByCodeHash).mockResolvedValue({
      title: 'Capacity History',
      series: [
        { key: 'used', label: 'Used', color: '#f59e0b' },
        { key: 'unused', label: 'Unused', color: '#00c389' },
      ],
      data: [
        {
          date: '2024-01-15',
          values: { used: '1000000000', unused: '1500000000' },
        },
      ],
    });
    vi.mocked(api.getCellsByScriptRef).mockResolvedValue(emptyCells);
  });

  it('renders unknown code hash with the unified script detail layout', async () => {
    render(<ScriptByCodeHashPage codeHash={mockCodeHash} />);

    await waitFor(() => {
      expect(screen.getByRole('table', { name: 'Code versions' })).toBeInTheDocument();
    });
    expect(screen.getByRole('heading', { name: 'Its cells' })).toBeInTheDocument();
    expect(screen.queryByText('Same Deployment References')).toBeNull();
    expect(screen.getByRole('heading', { name: 'Capacity history' })).toBeInTheDocument();
    expect(screen.getByText(/Live capacity and common knowledge across/)).toBeInTheDocument();
    expect(screen.getAllByText(mockCodeHash).length).toBeGreaterThan(0);
    expect(screen.getAllByRole('link', { name: `${mockDeploymentTxHash}:0` })[0]).toHaveAttribute(
      'href',
      `/mainnet/cell/${mockDeploymentTxHash}-0`
    );
  });

  // Rubbing × Song print, phase 5 (D6): an ambiguous reference's command runs on the number of
  // bytecode versions live type resolution points to.
  it('counts the conflicting bytecode versions of an ambiguous reference in its heading', async () => {
    const versions = [`0x${'aa'.repeat(32)}`, `0x${'bb'.repeat(32)}`];
    vi.mocked(api.lookupScripts).mockResolvedValue({
      [mockCodeHash]: {
        referenceHash: mockCodeHash,
        codeHash: mockCodeHash,
        name: 'Unknown',
        scriptKind: 'type',
        decoderType: null,
        hashType: 'type',
        deploymentTypeHash: mockCodeHash,
        deploymentDataHash: null,
        codeCellTxHash: null,
        codeCellOutputIndex: null,
        liveCellsCount: 0,
        ownedCapacitySum: '0',
        ownedKnowledgeSum: '0',
        codeCellsLiveCount: 2,
        codeCellsTotal: 2,
        resolutionState: 'ambiguous',
        ambiguity: { versionHashes: versions },
      },
    });

    render(<ScriptByCodeHashPage codeHash={mockCodeHash} />);

    expect(
      await screen.findByRole('heading', { level: 1, name: 'Ambiguous script reference' })
    ).toBeInTheDocument();
    const heading = screen.getByRole('heading', {
      name: 'Conflicting live bytecode 2 versions',
    });
    expect(heading.lastElementChild).toHaveTextContent(/^2 versions$/);
    for (const version of versions) expect(screen.getByText(version)).toBeInTheDocument();
  });

  it('redirects known script hash to the unified named script detail page', async () => {
    vi.mocked(api.lookupScripts).mockResolvedValue({
      [mockCodeHash]: {
        referenceHash: mockCodeHash,
        codeHash: mockCodeHash,
        name: 'Default Lock',
        scriptKind: 'lock',
        decoderType: null,
        hashType: 'type',
        deploymentTypeHash: mockCodeHash,
        deploymentDataHash: mockDataHash,
        codeCellTxHash: null,
        codeCellOutputIndex: null,
        liveCellsCount: 15,
        ownedCapacitySum: '25000000000',
        ownedKnowledgeSum: '14000000000',
        codeCellsLiveCount: 0,
        codeCellsTotal: 0,
        resolutionState: 'resolved',
        ambiguity: null,
      },
    });

    render(<ScriptByCodeHashPage codeHash={mockCodeHash} />);

    await waitFor(() => {
      expect(replaceMock).toHaveBeenCalledWith(
        `/scripts/${encodeURIComponent('Default Lock')}?ref=${mockCodeHash}&hashType=type&kind=lock`
      );
    });
  });

  // Readability F3-e: every column of the code reference's tables budgeted from what it holds, at
  // 0.6 em a glyph (the cells 13 px, units, legends and heads 12) and the 16 px gap after every
  // column but the last. A full id that breaks anywhere is a flexible column: at its minimum it
  // takes lines of 22 characters (a hash in three); its column takes the rest of the container.
  it('budgets every column of its tables from what it holds and changes form at their sums (F3-e)', async () => {
    const at = (text: string, px: number) => (text.length * px * 3) / 5; // 0.6 em, as textPx
    const ceil = (px: number) => Math.ceil(Math.round(px * 100) / 100);
    const sum = (budgets: number[]) => budgets.reduce((s, w) => s + w, 0);
    const idLine = ceil(at('x'.repeat(22), 13) + 16);
    const block = ceil(at('#99,999,999', 13) + 16);
    const versions = [
      idLine,
      block,
      // a count of four digits, wider than the head's words ("Code" / "cells")
      ceil(Math.max(at('9,999', 13), at('cells', 12)) + 16),
      // eight digits of live cells, or "unavailable"
      ceil(Math.max(at('99,999,999', 13), at('unavailable', 13)) + 16),
      // the compact CKB and its unit, "999.99M CKB", or "unavailable"; the last column
      ceil(Math.max(at('999.99M', 13) + at(' CKB', 12), at('unavailable', 13))),
    ];
    const deployments = [
      idLine,
      // "Consumed", or "Status unavailable" breaking between its words
      ceil(Math.max(at('Consumed', 13), at('unavailable', 13)) + 16),
      // the holder's evidence, in lines of 22 characters
      idLine,
      ceil(Math.max(at('#99,999,999', 13), at('unavailable', 13))),
    ];
    const cells = [
      idLine,
      idLine,
      idLine,
      // one line under a million CKB with its unit; a larger amount breaks before its decimals
      ceil(at('999,999.99999999', 13) + at(' CKB', 12) + 16),
      ceil(at('999,999.99999999', 13) + at(' CKB', 12) + 16),
      block,
      // the bar over its legend, whose widest part is its 8 px swatch, its 6 px gap and
      // "data 9,999,999 B"; the last column
      ceil(8 + 6 + at('data 9,999,999 B', 12)),
    ];
    expect([versions, deployments, cells]).toEqual([
      [188, 102, 55, 102, 86],
      [188, 102, 188, 86],
      [188, 188, 188, 170, 170, 102, 130],
    ]);
    expect([sum(versions), sum(deployments), sum(cells)]).toEqual([533, 564, 1136]);
    vi.mocked(api.getCellsByScriptRef).mockResolvedValue({
      ...emptyCells,
      total: 1,
      data: [
        {
          txHash: mockDeploymentTxHash,
          outputIndex: 1,
          capacity: '16100000000',
          commonKnowledgeSize: '6100000000',
          lockScriptHash: '0x01',
          dataSize: 0,
          createdAtBlock: 123456,
          status: 'live',
        },
      ],
    });
    const minWidths = (label: string) =>
      Array.from(screen.getByRole('table', { name: label }).querySelectorAll('col')).map((c) =>
        Number.parseFloat((c as HTMLElement).style.minWidth)
      );
    const formOf = async (label: string) =>
      (
        await waitFor(() => {
          const list =
            screen.queryByRole('table', { name: label }) ??
            screen.queryByRole('list', { name: `${label}, compact` });
          if (!list) throw new Error(`no ${label}`);
          return list;
        })
      )
        .closest('[data-list-form]')!
        .getAttribute('data-list-form');
    try {
      for (const [list, expected] of [
        [1232, ['table', 'table', 'table']],
        [1136, ['table', 'table', 'table']],
        [1135, ['table', 'table', 'rows']],
        [608, ['table', 'table', 'rows']],
        [358, ['rows', 'rows', 'rows']],
      ] as const) {
        stubLayoutWidths({ chart: 900, list });
        const { unmount } = render(<ScriptByCodeHashPage codeHash={mockCodeHash} />);
        const forms = [
          await formOf('Code versions'),
          await formOf('Code cell deployments'),
          await formOf('Its cells'),
        ];
        expect(forms, `at ${list}`).toEqual(expected);
        if (list === 1232) {
          expect(minWidths('Code versions')).toEqual(versions);
          expect(minWidths('Code cell deployments')).toEqual(deployments);
          expect(minWidths('Its cells')).toEqual(cells);
        }
        unmount();
        // the globals only: restoring the mocks would empty the api's (vitest 2), which this
        // page reads again at the next width
        vi.unstubAllGlobals();
      }
    } finally {
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    }
  });

  it('redirects /script/<name> alias to the named detail page', async () => {
    currentCodeHashParam = 'Default Lock';

    render(<ScriptByCodeHashPage codeHash="Default Lock" />);

    await waitFor(() => {
      expect(replaceMock).toHaveBeenCalledWith(`/scripts/${encodeURIComponent('Default Lock')}`);
    });
  });
});
