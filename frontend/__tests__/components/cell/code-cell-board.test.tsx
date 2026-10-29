import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { CodeCellBoard } from '@/components/cell/code-cell-board';
import type { Cell } from '@/lib/api';

const txA = `0x${'11'.repeat(32)}`;
const txB = `0x${'22'.repeat(32)}`;
const reference = `0x${'33'.repeat(32)}`;
const binaryA = `0x${'44'.repeat(32)}`;
const binaryB = `0x${'55'.repeat(32)}`;
const cell = {
  txHash: txB,
  outputIndex: 1,
  elf: {
    header: {
      class: 'ELF64',
      endian: 'little',
      machine: 243,
      entry: '9007199254740993',
      programHeaderOffset: '64',
      sectionHeaderOffset: '0',
      flags: 5,
      headerSize: 64,
      programHeaderEntrySize: 56,
      programHeaderCount: 1,
      sectionHeaderEntrySize: 0,
      sectionHeaderCount: 0,
      sectionNameIndex: 0,
    },
    programHeaders: [
      {
        index: 0,
        kind: 1,
        flags: 5,
        fileOffset: '128',
        fileSize: '16',
        virtualAddress: '9007199254740993',
        memorySize: '32',
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
    unassignedFileRanges: [{ start: 120, end: 128 }],
    comment: 'rustc 1.99',
    riscvAttributes: 'rv64imac',
  },
  codeCell: {
    versionHash: binaryB,
    currentReferences: [
      { referenceHash: reference, hashType: 'type', lockLiveCellsCount: 9, typeLiveCellsCount: 2 },
    ],
    typeIdVersions: [
      {
        typeScriptHash: reference,
        ordinal: 1,
        versionHash: binaryA,
        codeCellTxHash: txA,
        codeCellOutputIndex: 0,
        observedAtBlock: 100,
        reactivation: false,
        thisBinary: false,
      },
      {
        typeScriptHash: reference,
        ordinal: 2,
        versionHash: binaryB,
        codeCellTxHash: txB,
        codeCellOutputIndex: 1,
        observedAtBlock: 200,
        reactivation: false,
        thisBinary: true,
      },
      {
        typeScriptHash: reference,
        ordinal: 1,
        versionHash: binaryA,
        codeCellTxHash: txA,
        codeCellOutputIndex: 2,
        observedAtBlock: 300,
        reactivation: true,
        thisBinary: false,
      },
    ],
    upgradeCohort: [
      {
        previousTxHash: txA,
        previousOutputIndex: 0,
        currentTxHash: txB,
        currentOutputIndex: 1,
        typeScriptHash: reference,
        previousVersionHash: binaryA,
        currentVersionHash: binaryB,
        binaryChanged: true,
      },
    ],
  },
} as Cell;

describe('CodeCellBoard', () => {
  it('identifies this binary by canonical Type ID version ordinal and keeps reactivation at its original ordinal', () => {
    render(<CodeCellBoard cell={cell} />);
    expect(screen.getByText(/Version 2 · these bytes/)).toBeInTheDocument();
    expect(screen.getByText(/Version 1 · reactivated/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: `${txB}:1` })).toHaveAttribute(
      'href',
      `/mainnet/cell/${txB}-1`
    );
    expect(screen.getByText(/binary changed/)).toBeInTheDocument();
  });
  it('renders exact ELF file and memory ranges without losing wide addresses', () => {
    render(<CodeCellBoard cell={cell} />);
    expect(screen.getAllByText('0x20000000000001').length).toBeGreaterThan(0);
    expect(screen.getByText('Memory only')).toBeInTheDocument();
    expect(screen.getByText(/120–127 · 8 bytes · purpose unspecified/)).toBeInTheDocument();
    expect(screen.getByText(/rv64imac/)).toBeInTheDocument();
    expect(screen.getByText(/rustc 1.99/)).toBeInTheDocument();
  });
  it('labels ELF machine and header kinds by name instead of raw decimals', () => {
    render(<CodeCellBoard cell={cell} />);
    expect(screen.getByText(/ELF64 · little endian · RISC-V/)).toBeInTheDocument();
    expect(screen.getByText('LOAD')).toBeInTheDocument();
    expect(screen.getByText('NOBITS')).toBeInTheDocument();
    expect(screen.queryByText('machine 243')).not.toBeInTheDocument();
  });
  it('shows current reference counts as current evidence, not historical execution', () => {
    render(<CodeCellBoard cell={cell} />);
    expect(screen.getByText(/9 live locks · 2 live types/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: `type · ${reference}` })).toHaveAttribute(
      'href',
      `/mainnet/script/${reference}?hashType=type`
    );
    expect(screen.getByText(/actual cell deps/)).toBeInTheDocument();
  });
  it('does not label non-ELF payloads as code cells', () => {
    const { container } = render(<CodeCellBoard cell={{ ...cell, elf: undefined } as Cell} />);
    expect(container).toBeEmptyDOMElement();
  });
});
