import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { CellMicroscope, cellDataParts, cellOutputParts } from '@/components/cell/cell-microscope';
import { CodeCellBoard } from '@/components/cell/code-cell-board';
import type { Cell } from '@/lib/api';
import { sectionsWithManyHints } from '@/__tests__/utils/hints';
import { STICKY_RAIL } from '@/components/layout/sticky-header';
import { expectPrompt, promptNote } from '@/__tests__/utils/prompt-heading';
import { DATA_COLOR } from '@/lib/design-tokens';
import { hexRowBytes, hexRowWidth } from '@/lib/cell-byte-layout';
import codeCellFixture from '@/__tests__/fixtures/d5/cell-code-board.json';

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
  // Rubbing × Song print, phase 5 (A7): the bytes, run on the sum the map draws (its occupied total
  // in the data ink); the room left is the note, outside the heading's name.
  it('runs the bytes command on the occupied sum, the room left as its note', () => {
    render(<CellMicroscope cell={cell} />);
    const heading = screen.getByRole('heading', { name: 'Bytes 8 + 0 + 0 + 4 = 6' });
    const { args } = expectPrompt(heading, 'Bytes', '8 + 0 + 0 + 4 = 6');
    expect(within(args!).getByText('6')).toHaveStyle({ color: DATA_COLOR });
    expect(promptNote(heading)).toHaveTextContent(/^room for 94 more$/);
    expect(heading).not.toHaveTextContent('occupied bytes');
  });

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
    // a flat tile since rubbing × Song print: no lit top edge
    expect(box).toHaveClass('rounded-lg', 'border', 'border-base-border');
    expect(box).not.toHaveClass('neon-edge-top');
    expect(box).not.toHaveClass('border-base-border-subtle');
    const dump = box.querySelector('.overflow-x-auto') as HTMLElement;
    expect(dump).toHaveClass('px-4', 'py-3');
    // material pass v2 (move 2): the dump itself is a well inside the glass
    expect(dump).toHaveClass('well', 'm-2');
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

// Old issue B (2026-11-12): from lg the rail beside the bytes had a 0 minimum, so at 1024 px it
// got only what the bytes' 846 left (976 − 24 − 846 = 106 px, 76 of text), and every detail
// head's unbroken figure ran 27–47 px past the page. The rail now keeps 320 px, and each of the
// four heads lets its figure wrap under the name as one unit.
describe('cell microscope field detail rail (old issue B)', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });
  const HEAD = 'flex flex-wrap items-baseline justify-between gap-x-2 gap-y-0.5';
  const NAME = 'text-text-bright text-body min-w-0 break-words';
  const FIGURE = 'text-text-dim text-meta whitespace-nowrap';
  const elfCell = {
    ...codeCellFixture,
    data: `0x${'7f454c46020101000000000000000000' + '0200' + 'f300' + '01000000'}${'00'.repeat(
      100312 - 24
    )}`,
  } as unknown as Cell;

  /** The rail's one head: the name that may wrap between words, then the figure as one unit. */
  function expectHead(name: string, figure: string) {
    const rail = screen.getByRole('complementary', { name: 'Field detail' });
    const heads = rail.querySelectorAll<HTMLElement>('[data-detail-head]');
    expect(heads).toHaveLength(1);
    expect(heads[0].className).toBe(HEAD);
    expect([...heads[0].children].map((el) => [el.tagName, el.className, el.textContent])).toEqual([
      ['SPAN', NAME, name],
      ['SPAN', FIGURE, figure],
    ]);
  }

  it('keeps the rail at least 320 px beside the bytes from lg, the bytes up to 846', () => {
    render(<CellMicroscope cell={cell} />);
    const grid = screen.getByRole('complementary', { name: 'Field detail' }).parentElement!;
    expect(grid.className).toBe(
      'grid items-start gap-6 pt-2 lg:grid-cols-[minmax(0,846px)_minmax(320px,1fr)]'
    );
    // below lg one column: the rail under the bytes at the full width
    expect(grid.className).not.toMatch(/(^|\s)grid-cols-/);
  });

  it('lets the default head’s count wrap under its name', () => {
    render(<CellMicroscope cell={cell} />);
    expectHead('Where each byte belongs', '6 + 4 bytes');
  });

  it('lets a field’s range wrap under its name', () => {
    render(<CellMicroscope cell={cell} />);
    fireEvent.mouseOver(screen.getByLabelText('Cell bytes').querySelector('[data-byte="data:2"]')!);
    expectHead('unclassified bytes', '[2..4) · 2 B');
    fireEvent.mouseEnter(screen.getByRole('button', { name: /^code_hash \[4\.\.6\)/ }));
    expectHead('lock · code_hash', '[4..6) · 2 B');
  });

  it('lets a map part’s occupied size wrap under its name and script', () => {
    const locked = {
      ...cell,
      commonKnowledgeSizeBreakdown: { ...cell.commonKnowledgeSizeBreakdown!, lockScriptBytes: 2 },
    } as Cell;
    render(<CellMicroscope cell={locked} lockName="Multisig" />);
    const map = screen.getByRole('group', { name: 'Capacity map' });
    fireEvent.mouseEnter(within(map).getByTitle('lock · 2 bytes'));
    expectHead('lock · Multisig', '2 B occupied');
    fireEvent.mouseEnter(within(map).getByTitle('data · 4 bytes'));
    expectHead('data', '4 B occupied');
  });

  it('lets an ELF region’s range wrap under its name', () => {
    render(<CellMicroscope cell={elfCell} />);
    const map = screen.getByRole('group', { name: 'ELF binary map' });
    fireEvent.mouseEnter(within(map).getByTitle('.text · 91,352 B'));
    const text = screen
      .getByRole('complementary', { name: 'Field detail' })
      .querySelector('[data-detail-head] > :last-child')!.textContent!;
    expect(text).toMatch(/^\[[\d,]+\.\.[\d,]+\) · 91,352 B$/);
    expectHead('.text', text);
  });

  it('budgets 1024 px: the bytes hold a whole 16-byte row, the rail its widest figure', () => {
    render(<CellMicroscope cell={elfCell} />);
    const grid = screen.getByRole('complementary', { name: 'Field detail' }).parentElement!;
    const rail = Number(/_minmax\((\d+)px,1fr\)\]/.exec(grid.className)![1]);
    expect(rail).toBe(320);
    // 1024 − the container's 2 × 24 px gutter = 976; the grid's gap-6 is 24
    const bytes = 976 - 24 - rail;
    expect(bytes).toBe(632);
    // the dump's frame: the panel's 1 px border, its well's m-2, the well's 1 px border and px-4
    const frame = 2 * (1 + 8 + 1 + 16);
    for (const digits of [4, 5] as const) {
      expect(hexRowBytes(bytes, digits)).toBe(16);
      expect(hexRowWidth(16, digits) + frame).toBeLessThanOrEqual(bytes);
    }
    // the rail's text: 320 − its 1 px border and px-3.5 on each side = 290 px; every figure of the
    // 100,312-byte code cell (11 px, 6.6 px a character) is one unit inside it
    const text = rail - 2 * (1 + 14);
    const parts = [...cellOutputParts(elfCell), ...cellDataParts(elfCell)];
    const widest = Math.max(
      ...parts.map(
        (p) =>
          `[${p.start.toLocaleString('en-US')}..${p.end.toLocaleString('en-US')}) · ${(p.end - p.start).toLocaleString('en-US')} B`
            .length
      )
    );
    expect(widest * 6.6).toBeLessThanOrEqual(text);
    // and at 1280 (1,232 of content) the bytes keep their 846 and the rail its 362, as before
    expect(1232 - 24 - 846).toBeGreaterThanOrEqual(rail);
  });
});

// Visual finish V11: the dump's inks are theme classes or named inks, no bracket hex.
describe('cell microscope inks (V11)', () => {
  it('draws the offsets in the quiet slate and the rail on the theme’s row rule', () => {
    stubChartWidth(1200);
    render(<CellMicroscope cell={cell} />);
    const box = screen.getByLabelText('Cell bytes');
    const offsets = [...box.querySelectorAll<HTMLElement>('span')].filter((el) =>
      /^0x[0-9a-f]{4,5}$/.test(el.textContent ?? '')
    );
    expect(offsets.length).toBeGreaterThan(0);
    for (const el of offsets) {
      expect(el).toHaveClass('text-[color:var(--ink)]');
      expect(el.style.getPropertyValue('--ink')).toBe('var(--c-ink-quiet)');
    }
    expect(screen.getByRole('complementary', { name: 'Field detail' })).toHaveClass(
      'border-base-border-subtle'
    );
    expect(document.body.innerHTML).not.toMatch(/(text|bg|border)-\[#/);
  });
});
