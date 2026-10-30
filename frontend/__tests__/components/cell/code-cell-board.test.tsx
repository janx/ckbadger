import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { CodeCellBoard } from '@/components/cell/code-cell-board';
import { elfParts } from '@/lib/cell-parts';
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
  it('reads wide ELF addresses exactly and names kinds instead of raw decimals', () => {
    // 64 header + 56 program header + 8 unassigned + 16 loaded: the bytes the layout covers.
    const parts = elfParts({ ...cell, data: `0x${'00'.repeat(144)}` } as Cell);
    const load = parts.find((part) => part.id === 'elf:ph0')!;
    expect(load.label).toBe('program header 0 · LOAD');
    expect(load.value).toBe('RX · file [128..144) → 0x20000000000001 · memsz 32');
    expect(parts.find((part) => part.label === 'e_machine')!.value).toBe('RISC-V');
    expect(parts.find((part) => part.label === 'e_entry')!.value).toBe('0x20000000000001');
    // NOBITS sections have no file bytes, so they own none
    expect(parts.some((part) => part.label === '.bss')).toBe(false);
    expect(parts.find((part) => part.start === 120)).toMatchObject({
      label: 'alignment padding',
      end: 128,
    });
    expect(parts.find((part) => part.start === 128)).toMatchObject({
      label: 'padding in program segment 0',
      end: 144,
    });
    render(<CodeCellBoard cell={cell} />);
    expect(screen.getByTestId('code-built-with')).toHaveTextContent('rustc 1.99');
    expect(screen.getByTestId('code-built-with')).toHaveTextContent(
      'target rv64imac · entry 0x20000000000001 · no symbol table'
    );
    expect(screen.queryByRole('table')).toBeNull();
  });
  it('counts the live cells that name these bytes by each way, as current evidence', () => {
    render(
      <CodeCellBoard
        cell={{
          ...cell,
          cellOutputMolecule: {
            bytes: '0x',
            fields: [],
            lockScriptHash: `0x${'66'.repeat(32)}`,
            typeScriptHash: reference,
          },
        }}
      />
    );
    expect(screen.getByTestId('code-by-type-hash')).toHaveTextContent(
      '11 live cells name it this way'
    );
    expect(screen.getByTestId('code-by-data-hash')).toHaveTextContent(
      '0 live cells pin this exact binary'
    );
  });
  it('lists a reference that names neither the type hash nor these bytes on its own', () => {
    render(<CodeCellBoard cell={cell} />);
    expect(screen.getByRole('link', { name: `type · ${reference}` })).toHaveAttribute(
      'href',
      `/mainnet/script/${reference}?hashType=type`
    );
    expect(screen.getByText(/· 11 live cells/)).toBeInTheDocument();
  });
  it('does not label non-ELF payloads as code cells', () => {
    const { container } = render(<CodeCellBoard cell={{ ...cell, elf: undefined } as Cell} />);
    expect(container).toBeEmptyDOMElement();
  });
});
