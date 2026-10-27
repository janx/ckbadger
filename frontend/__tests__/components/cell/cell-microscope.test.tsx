import { describe, expect, it } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { CellMicroscope, dataFields, moleculeFields } from '@/components/cell/cell-microscope';
import { CodeCellBoard } from '@/components/cell/code-cell-board';
import type { Cell } from '@/lib/api';

const cell = {
  txHash: `0x${'01'.repeat(32)}`,
  outputIndex: 0,
  capacity: '10000000000',
  commonKnowledgeSize: '6100000000',
  lockScriptHash: `0x${'11'.repeat(32)}`,
  dataSize: 4,
  createdAtBlock: 1,
  data: '0x10000102',
  dataAnalysis: {
    deterministic: {
      kind: 'fixture',
      summary: 'fixture',
      segments: [
        { label: 'length_word', start: 0, end: 2, meaning: 'Molecule length', humanValue: '16' },
      ],
    },
    heuristicGuesses: [],
  },
  cellOutputMolecule: {
    bytes: '0x04000000aabb',
    fields: [
      { label: 'total_size', start: 0, end: 4, occupied: false },
      { label: 'lock.code_hash', start: 4, end: 6, occupied: true },
    ],
    lockScriptHash: `0x${'11'.repeat(32)}`,
    typeScriptHash: null,
  },
} as Cell;

describe('cell microscope', () => {
  it('partitions serialized output and fills only unclassified data gaps', () => {
    expect(moleculeFields(cell).map((field) => [field.offset, field.length])).toEqual([
      [0, 4],
      [4, 2],
    ]);
    expect(dataFields(cell).map((field) => [field.label, field.offset, field.length])).toEqual([
      ['length word', 0, 2],
      ['Unclassified bytes', 2, 2],
    ]);
    render(<CellMicroscope cell={cell} />);
    expect(screen.getByLabelText('CellOutput byte map')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'CellData' }));
    expect(screen.getByLabelText('CellData byte map')).toBeInTheDocument();
  });

  it('rejects an overlapping or incomplete byte map', () => {
    const broken = {
      ...cell,
      cellOutputMolecule: {
        ...cell.cellOutputMolecule!,
        fields: [{ label: 'bad', start: 1, end: 4, occupied: false }],
      },
    } as Cell;
    expect(() => moleculeFields(broken)).toThrow('does not partition');
  });
});

it('shows NOBITS as memory only and current references as current evidence', () => {
  const codeCell = {
    ...cell,
    elf: {
      header: {
        class: 'ELF64',
        endian: 'little',
        machine: 243,
        entry: '4096',
        programHeaderOffset: '64',
        sectionHeaderOffset: '128',
        flags: 0,
        headerSize: 64,
        programHeaderEntrySize: 56,
        programHeaderCount: 1,
        sectionHeaderEntrySize: 64,
        sectionHeaderCount: 1,
        sectionNameIndex: 0,
      },
      programHeaders: [
        {
          index: 0,
          kind: 1,
          flags: 5,
          fileOffset: '256',
          fileSize: '8',
          virtualAddress: '4096',
          memorySize: '16',
          alignment: '8',
        },
      ],
      sections: [
        {
          index: 0,
          name: '.bss',
          kind: 8,
          flags: '3',
          address: '8192',
          fileOffset: null,
          fileSize: null,
          memorySize: '16',
          alignment: '8',
        },
      ],
      unassignedFileRanges: [{ start: 192, end: 256 }],
      comment: 'rustc',
      riscvAttributes: 'rv64imac',
    },
    codeCell: {
      versionHash: `0x${'22'.repeat(32)}`,
      currentReferences: [
        {
          referenceHash: `0x${'33'.repeat(32)}`,
          hashType: 'data1',
          lockLiveCellsCount: 7,
          typeLiveCellsCount: 2,
        },
      ],
      typeIdLineage: [],
      upgradeCohort: [],
    },
  } as Cell;
  render(<CodeCellBoard cell={codeCell} />);
  expect(screen.getByText('Memory only')).toBeInTheDocument();
  expect(screen.getByText(/7 live locks/)).toBeInTheDocument();
  expect(screen.getByText(/actual cell deps/)).toBeInTheDocument();
});

it('pages the complete byte stream and jumps to the last ELF file range', () => {
  const large = {
    ...cell,
    data: `0x${'ab'.repeat(1200)}`,
    dataSize: 1200,
    dataAnalysis: undefined,
  } as Cell;
  render(<CellMicroscope cell={large} />);
  fireEvent.click(screen.getByRole('button', { name: 'CellData' }));
  expect(screen.getByText(/page 1\/3/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: 'Next bytes' }));
  fireEvent.click(screen.getByRole('button', { name: 'Next bytes' }));
  expect(screen.getByText(/1024–1199 of 1200/)).toBeInTheDocument();
  expect(screen.getByLabelText('CellData byte map')).toBeInTheDocument();
});

it('links actual same-transaction Type ID upgrades without treating unchanged bytes as upgrades', () => {
  const previous = `0x${'44'.repeat(32)}`;
  const current = `0x${'55'.repeat(32)}`;
  const codeCell = {
    ...cell,
    elf: {
      header: {
        class: 'ELF64',
        endian: 'little',
        machine: 243,
        entry: '4096',
        programHeaderOffset: '0',
        sectionHeaderOffset: '0',
        flags: 0,
        headerSize: 64,
        programHeaderEntrySize: 0,
        programHeaderCount: 0,
        sectionHeaderEntrySize: 0,
        sectionHeaderCount: 0,
        sectionNameIndex: 0,
      },
      programHeaders: [],
      sections: [],
      unassignedFileRanges: [],
      comment: null,
      riscvAttributes: null,
    },
    codeCell: {
      versionHash: `0x${'66'.repeat(32)}`,
      currentReferences: [],
      typeIdLineage: [{ typeScriptHash: `0x${'77'.repeat(32)}`, observedAtBlock: 123 }],
      upgradeCohort: [
        {
          previousTxHash: previous,
          previousOutputIndex: 3,
          currentTxHash: current,
          currentOutputIndex: 1,
          typeScriptHash: `0x${'77'.repeat(32)}`,
          previousVersionHash: `0x${'66'.repeat(32)}`,
          currentVersionHash: `0x${'66'.repeat(32)}`,
          binaryChanged: false,
        },
      ],
    },
  } as Cell;
  render(<CodeCellBoard cell={codeCell} />);
  expect(screen.getByText(/same binary/)).toBeInTheDocument();
  expect(screen.getByRole('link', { name: /Input/ })).toHaveAttribute(
    'href',
    `/mainnet/cell/${previous}-3`
  );
  expect(screen.getByRole('link', { name: /output #1/ })).toHaveAttribute(
    'href',
    `/mainnet/cell/${current}-1`
  );
});
