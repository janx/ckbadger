import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { CellMicroscope, cellDataParts, cellOutputParts } from '@/components/cell/cell-microscope';
import { CodeCellBoard } from '@/components/cell/code-cell-board';
import type { Cell } from '@/lib/api';
import { sectionsWithManyHints } from '@/__tests__/utils/hints';
import { STICKY_RAIL } from '@/components/layout/sticky-header';

const cell = {
  txHash: `0x${'01'.repeat(32)}`,
  outputIndex: 0,
  capacity: '10000000000',
  commonKnowledgeSize: '600000000',
  commonKnowledgeSizeBreakdown: {
    capacityFieldBytes: 8,
    lockScriptBytes: 0,
    typeScriptBytes: 0,
    dataBytes: 4,
    totalBytes: 6,
  },
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
  it('sticks the field detail one gutter under the measured site header, not a fixed offset', () => {
    // A long hex dump scrolls far; the detail must stay visible below the header, never under it.
    render(<CellMicroscope cell={cell} />);
    const aside = screen.getByLabelText('Field detail');
    expect(aside).toHaveClass(...STICKY_RAIL.split(' '));
    expect(STICKY_RAIL).toContain('var(--sticky-header-h)');
    expect(aside.className).not.toMatch(/\blg:top-\d/);
  });

  it('partitions serialized output and fills only unclassified data gaps', () => {
    expect(cellOutputParts(cell).map((part) => [part.start, part.end, part.occupied])).toEqual([
      [0, 4, false],
      [4, 6, true],
    ]);
    expect(cellDataParts(cell).map((part) => [part.label, part.start, part.end])).toEqual([
      ['length word', 0, 2],
      ['unclassified bytes', 2, 4],
    ]);
    render(<CellMicroscope cell={cell} />);
    expect(screen.getByLabelText('CellOutput byte map')).toBeInTheDocument();
    expect(screen.getByLabelText('CellData byte map')).toBeInTheDocument();
  });

  // Lane B (B1): the API describes an ELF it cannot parse; the map says so and draws no layout.
  it('says a data ELF does not parse and leaves its bytes unclassified', () => {
    const elfCell = {
      ...cell,
      data: '0x7f454c46',
      dataAnalysis: undefined,
      elf: { status: 'unparsed', reason: 'ELF header truncated: 4 bytes' },
    } as Cell;
    render(<CellMicroscope cell={elfCell} />);
    const data = screen.getByLabelText('CellData byte map');
    expect(data).toHaveTextContent(
      'outputs_data[0] · an ELF file that does not parse: ELF header truncated: 4 bytes'
    );
    expect(cellDataParts(elfCell).map((part) => part.label)).toEqual(['unclassified bytes']);
  });

  // Midnight, continued (review 2026-11-03, item 5, move 3): the byte dump is an instrument, so
  // it sits in the one frame (TerminalPanel); its handlers and the highlight's data attributes
  // stay on the frame, the dump scrolls sideways inside it, and the field-detail rail stays flat.
  it('puts the byte dump in the instrument frame, with its handlers and highlight hooks (move 3)', () => {
    stubChartWidth(1200);
    render(<CellMicroscope cell={cell} />);
    const box = screen.getByLabelText('Cell bytes');
    expect(box).toHaveAttribute('data-instrument-frame');
    expect(box).toHaveAttribute('data-cell-bytes');
    expect(box).toHaveClass('neon-edge-top', 'rounded-md', 'border', 'border-base-border');
    expect(box).not.toHaveClass('border-[#1a1f30]');
    const dump = box.querySelector('.overflow-x-auto') as HTMLElement;
    expect(dump).toHaveClass('px-4', 'py-3');
    expect(dump).toContainElement(box.querySelector('[data-byte="data:2"]') as HTMLElement);
    // the hover still lights the part through the frame's data-active
    fireEvent.mouseOver(box.querySelector('[data-byte="data:2"]')!);
    expect(box.getAttribute('data-active')).toBeTruthy();
    expect(screen.getByRole('complementary', { name: 'Field detail' })).not.toHaveAttribute(
      'data-instrument-frame'
    );
  });

  it('a tap pins a byte, a field or a map part in the line under the bytes; a second tap clears it (R6)', () => {
    render(<CellMicroscope cell={cell} />);
    const box = screen.getByLabelText('Cell bytes');
    const readout = screen.getByRole('status', { name: 'Byte readout' });
    const detail = screen.getByRole('complementary', { name: 'Field detail' });
    expect(box.compareDocumentPosition(readout) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(readout).toHaveTextContent(/^tap or hover$/);
    // a tap on a byte: the emulated hover, then the click; the pin outlasts the pointer
    const byte = box.querySelector('[data-byte="data:2"]')!;
    fireEvent.mouseOver(byte);
    fireEvent.click(byte);
    fireEvent.mouseLeave(box);
    expect(readout).toHaveTextContent(/^selected unclassified bytes · \[2\.\.4\) · 2 B/);
    expect(detail).toHaveTextContent('unclassified bytes');
    expect(box).toHaveAttribute('data-active', 'data:unclassified-2');
    // a tap on a field pill or a map part moves the pin there
    fireEvent.click(screen.getByRole('button', { name: /^length word \[0\.\.2\)/ }));
    expect(readout).toHaveTextContent(/^selected length word · \[0\.\.2\) · 2 B · 16/);
    const map = screen.getByRole('group', { name: 'Capacity map' });
    fireEvent.click(within(map).getByTitle('data · 4 bytes'));
    fireEvent.mouseLeave(map);
    expect(readout).toHaveTextContent(/^selected data · 4 B occupied/);
    expect(within(map).getByTitle('data · 4 bytes')).toHaveAttribute('aria-pressed', 'true');
    // the second tap on it lets go, though the emulated hover is still on it
    fireEvent.mouseEnter(within(map).getByTitle('data · 4 bytes'));
    fireEvent.click(within(map).getByTitle('data · 4 bytes'));
    expect(readout).toHaveTextContent(/^tap or hover$/);
    // C10: the readout row's "tap or hover" is the section's one hint; the detail box names
    // what it holds by default.
    expect(detail).toHaveTextContent(/^Where each byte belongs/);
    expect(detail).not.toHaveTextContent(/tap|hover/i);
  });

  it('says "tap or hover" once in the microscope (C10)', () => {
    const { container } = render(
      <section aria-label="Cell byte microscope">
        <CellMicroscope cell={cell} />
      </section>
    );
    expect(sectionsWithManyHints(container)).toEqual([]);
    expect(container.querySelectorAll('[data-hint="tap-or-hover"]')).toHaveLength(1);
  });

  it('rejects an overlapping or incomplete byte map', () => {
    const broken = {
      ...cell,
      cellOutputMolecule: {
        ...cell.cellOutputMolecule!,
        fields: [{ label: 'bad', start: 1, end: 4, occupied: false }],
      },
    } as Cell;
    expect(() => cellOutputParts(broken)).toThrow('does not partition');
  });

  it('fails visibly when the bytes disagree with the occupied size', () => {
    render(
      <CellMicroscope
        cell={{
          ...cell,
          commonKnowledgeSizeBreakdown: { ...cell.commonKnowledgeSizeBreakdown!, totalBytes: 7 },
        }}
      />
    );
    expect(screen.getByRole('alert')).toHaveTextContent('disagree with its occupied size 7 B');
  });
});

it('shows NOBITS as memory only and current references as current evidence', () => {
  const codeCell = {
    ...cell,
    elf: {
      status: 'parsed',
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
      riscvAttributes: { status: 'parsed', arch: 'rv64imac' },
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
      typeIdVersions: [],
      upgradeCohort: [],
    },
  } as Cell;
  render(<CodeCellBoard cell={codeCell} />);
  // a data1 reference to another binary is listed with its live users, never as these bytes'
  expect(screen.getByText(/· 9 live cells/)).toBeInTheDocument();
  expect(screen.getByTestId('code-by-data-hash')).toHaveTextContent(
    '0 live cells pin this exact binary'
  );
});

it('shows long data as excerpts with "⋯ N bytes … show" rows instead of pages (SC-K4)', () => {
  const large = {
    ...cell,
    capacity: '400200000000',
    data: `0x${'ab'.repeat(4000)}`,
    dataSize: 4000,
    dataAnalysis: undefined,
    commonKnowledgeSizeBreakdown: {
      ...cell.commonKnowledgeSizeBreakdown!,
      dataBytes: 4000,
      totalBytes: 4002,
    },
  } as Cell;
  render(<CellMicroscope cell={large} />);
  expect(screen.queryByRole('button', { name: 'Next bytes' })).toBeNull();
  const data = screen.getByLabelText('CellData byte map');
  expect(data.querySelectorAll('[data-byte^="data:"]')).toHaveLength(768 + 400);
  expect(data).toHaveTextContent('⋯ 2,832 bytes [768..3,600): unclassified bytes');
  fireEvent.click(screen.getByRole('button', { name: 'show' }));
  expect(data.querySelectorAll('[data-byte^="data:"]')).toHaveLength(768 + 1032 + 400);
  expect(data).toHaveTextContent('⋯ 1,800 bytes [1,800..3,600)');
});

it('links actual same-transaction Type ID upgrades without treating unchanged bytes as upgrades', () => {
  const previous = `0x${'44'.repeat(32)}`;
  const current = `0x${'55'.repeat(32)}`;
  const codeCell = {
    ...cell,
    elf: {
      status: 'parsed',
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
      typeIdVersions: [
        {
          typeScriptHash: `0x${'77'.repeat(32)}`,
          ordinal: 5,
          versionHash: `0x${'66'.repeat(32)}`,
          codeCellTxHash: current,
          codeCellOutputIndex: 1,
          observedAtBlock: 123,
          reactivation: false,
          thisBinary: true,
        },
      ],
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
  expect(screen.getByText(/Version 5 · these bytes/)).toBeInTheDocument();
  expect(screen.getByRole('link', { name: /Input/ })).toHaveAttribute(
    'href',
    `/mainnet/cell/${previous}-3`
  );
  expect(screen.getByRole('link', { name: /output #1/ })).toHaveAttribute(
    'href',
    `/mainnet/cell/${current}-1`
  );
});

describe('cell microscope at phone width (responsive review A#10, A#11)', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });
  const wide = {
    ...cell,
    data: `0x${'ab'.repeat(40)}`,
    dataSize: 40,
    dataAnalysis: undefined,
    commonKnowledgeSizeBreakdown: {
      capacityFieldBytes: 8,
      lockScriptBytes: 0,
      typeScriptBytes: 0,
      dataBytes: 40,
      totalBytes: 42,
    },
  } as Cell;

  it('lays the hex dump out 8 bytes a row in a 358 px box, its headers wrapping as wholes', () => {
    stubChartWidth(358);
    render(<CellMicroscope cell={wide} />);
    const data = screen.getByLabelText('CellData byte map');
    const offsets = within(data)
      .getAllByText(/^0x[0-9a-f]{4}$/)
      .map((n) => n.textContent);
    expect(offsets).toEqual(['0x0000', '0x0008', '0x0010', '0x0018', '0x0020']);
    // Each header is one wrapping row of two parts, never two ragged columns.
    for (const block of ['CellOutput byte map', 'CellData byte map'])
      expect(screen.getByLabelText(block).firstElementChild).toHaveClass('flex-wrap');
  });

  it('lays the capacity map out so its gaps fit and a 2-byte part is a whole box', () => {
    const small = {
      ...cell,
      data: '0x1000',
      dataSize: 2,
      dataAnalysis: undefined,
      commonKnowledgeSizeBreakdown: {
        capacityFieldBytes: 8,
        lockScriptBytes: 0,
        typeScriptBytes: 0,
        dataBytes: 2,
        totalBytes: 4,
      },
    } as Cell;
    render(<CellMicroscope cell={small} />);
    const map = screen.getByRole('group', { name: 'Capacity map' });
    // A flex row of basis-0 parts: the 2 px gaps are inside the row's width.
    expect(map).toHaveClass('flex');
    expect(map.style.gap).toBe('2px');
    const parts = [...map.querySelectorAll<HTMLElement>('[data-map]')];
    expect(parts.map((p) => [p.dataset.map, p.style.flex])).toEqual([
      ['capacity', '8 0 0px'],
      ['data', '2 0 0px'],
    ]);
    // The 2-byte part keeps the width its "2" needs inside its padding.
    const data = parts[1];
    expect(data).toHaveTextContent(/^2$/);
    expect(data.style.paddingInline).toBe('4px');
    expect(Number.parseFloat(data.style.minWidth)).toBeGreaterThanOrEqual(6.3 + 8);
  });
});
