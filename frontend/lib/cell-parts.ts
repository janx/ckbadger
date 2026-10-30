import type { Cell } from '@/lib/api';
import { hexBytes } from '@/lib/hex-bytes';
import { elfMachineLabel, elfProgramTypeLabel, elfTypeLabel } from '@/lib/elf-labels';
import { CELL_COLORS, ELF_REGION_COLORS } from '@/lib/design-tokens';

/**
 * Every byte of a cell, named once: its CellOutput (molecule, from `cellOutputMolecule`) and its
 * data (the API's deterministic decode, or the ELF layout the API parsed). The names and value
 * readings come from the API and the fixed CellOutput / ELF64 layouts; nothing is guessed, and a
 * range set that does not cover the bytes exactly throws.
 */
export type CellGroup = 'capacity' | 'lock' | 'type' | 'data';
export interface CellPart {
  id: string;
  block: 'output' | 'data';
  group: CellGroup;
  /** Full name (`lock · code_hash`, `e_entry`, `.text`). */
  label: string;
  start: number;
  end: number;
  occupied: boolean;
  tone: string;
  /** Molecule / ELF type of the field, when the layout fixes one. */
  type: string | null;
  value: string | null;
  meaning: string;
  /** Listed in the pill index (the rest are reached through their bytes). */
  indexed: boolean;
}

const FRAMING = '#4b5266';
/** SporeData-style framing inside the data: occupied, but structure rather than content. */
const DATA_FRAMING = '#5f9fb2';

function u32le(bytes: readonly string[], at: number): number {
  return Number.parseInt(
    bytes
      .slice(at, at + 4)
      .reverse()
      .join(''),
    16
  );
}

function moleculeType(field: string): string {
  if (field === 'capacity') return 'Uint64';
  if (field.endsWith('code_hash')) return 'Byte32';
  if (field.endsWith('hash_type')) return 'byte';
  if (field.endsWith('.args')) return 'Bytes';
  return 'u32';
}

const HASH_TYPE_MEANING: Record<string, string> = {
  type: 'find the code by type hash',
  data: 'find the code by data hash, CKB-VM version 0',
  data1: 'find the code by data hash, CKB-VM version 1',
  data2: 'find the code by data hash, CKB-VM version 2',
};

/** The CellOutput's bytes, field by field, as `cellOutputMolecule` partitions them. */
export function cellOutputParts(cell: Cell): CellPart[] {
  const molecule = cell.cellOutputMolecule;
  if (!molecule) return [];
  const bytes = hexBytes(molecule.bytes, 'CellOutput');
  let end = 0;
  return molecule.fields.map((field) => {
    if (field.start !== end || field.end <= field.start || field.end > bytes.length)
      throw new Error(
        `CellOutput field map: ${field.label} does not partition ${bytes.length} bytes`
      );
    end = field.end;
    const [scope, name] = field.label.includes('.')
      ? field.label.split('.', 2)
      : [null, field.label];
    const group: CellGroup = scope === 'lock' ? 'lock' : scope === 'type' ? 'type' : 'capacity';
    const script = scope === 'lock' ? cell.lock : scope === 'type' ? cell.type : undefined;
    let value: string | null = null;
    let meaning: string;
    if (!field.occupied) {
      value = field.end - field.start === 4 ? String(u32le(bytes, field.start)) : null;
      meaning = name.endsWith('length')
        ? 'Length prefix of the args Bytes. Molecule framing, not occupied.'
        : 'Where a field starts, or how long the table is. Molecule framing, not occupied.';
    } else if (field.label === 'capacity') {
      value = `${BigInt(cell.capacity).toLocaleString('en-US')} shannons`;
      meaning = 'The CKB the cell holds, little-endian. It must cover every occupied byte.';
    } else if (name === 'code_hash') {
      value = script?.codeHash ?? null;
      meaning = `Which code runs as the ${scope} script, matched as its hash type says.`;
    } else if (name === 'hash_type') {
      value = script ? `0x${bytes[field.start]} = ${script.hashType}` : `0x${bytes[field.start]}`;
      meaning = script
        ? `How the code hash is matched: ${HASH_TYPE_MEANING[script.hashType] ?? script.hashType}.`
        : 'How the code hash is matched.';
    } else if (name === 'args') {
      value = script?.args ?? null;
      meaning = `The arguments the ${scope} script reads.`;
    } else {
      meaning = 'Occupied CellOutput field.';
    }
    return {
      id: `out:${field.label}`,
      block: 'output',
      group,
      label: scope ? `${scope} · ${name}` : name,
      start: field.start,
      end: field.end,
      occupied: field.occupied,
      tone: field.occupied ? CELL_COLORS[group] : FRAMING,
      type: moleculeType(field.label),
      value,
      meaning,
      indexed: true,
    };
  });
}

/** Data bytes named by the API's deterministic decode; bytes it does not label stay "unclassified". */
function decodedDataParts(cell: Cell, length: number): CellPart[] {
  const segments = [...(cell.dataAnalysis?.deterministic?.segments ?? [])].sort(
    (a, b) => a.start - b.start || a.end - b.end
  );
  const parts: CellPart[] = [];
  let end = 0;
  const gap = (from: number, to: number) =>
    parts.push({
      id: `data:unclassified-${from}`,
      block: 'data',
      group: 'data',
      label: 'unclassified bytes',
      start: from,
      end: to,
      occupied: true,
      tone: CELL_COLORS.data,
      type: null,
      value: null,
      meaning: 'Exact bytes with no deterministic protocol label. Occupied, like every data byte.',
      indexed: true,
    });
  for (const segment of segments) {
    if (segment.start < end || segment.end <= segment.start || segment.end > length)
      throw new Error(`CellData segment ${segment.label} overlaps or exceeds ${length} bytes`);
    if (segment.start > end) gap(end, segment.start);
    const framing = /length|offset|total_size|header/i.test(segment.label);
    parts.push({
      id: `data:${segment.label}-${segment.start}`,
      block: 'data',
      group: 'data',
      label: segment.label.replaceAll('_', ' '),
      start: segment.start,
      end: segment.end,
      occupied: true,
      tone: framing ? DATA_FRAMING : CELL_COLORS.data,
      type: null,
      value: segment.humanValue,
      meaning: `${segment.meaning}. Inside the data, so it is occupied.`,
      indexed: true,
    });
    end = segment.end;
  }
  if (end < length) gap(end, length);
  return parts;
}

const ELF64_HEADER: readonly [string, number, number, string][] = [
  ['e_ident · magic', 0, 4, 'This data is an ELF file.'],
  ['e_ident · class', 4, 5, '64-bit or 32-bit.'],
  ['e_ident · data', 5, 6, 'Byte order.'],
  ['e_ident · version', 6, 7, 'ELF version.'],
  ['e_ident · OS ABI', 7, 8, 'Operating system ABI.'],
  ['e_ident · padding', 8, 16, 'Unused.'],
  ['e_type', 16, 18, 'What kind of object file it is.'],
  ['e_machine', 18, 20, 'The instruction set. CKB-VM runs RISC-V.'],
  ['e_version', 20, 24, 'ELF version.'],
  ['e_entry', 24, 32, 'Where CKB-VM starts running.'],
  ['e_phoff', 32, 40, 'Program headers start here.'],
  ['e_shoff', 40, 48, 'Section headers start here.'],
  ['e_flags', 48, 52, 'Processor flags (RISC-V: compressed instructions, float ABI).'],
  ['e_ehsize', 52, 54, 'Size of this header.'],
  ['e_phentsize', 54, 56, 'Size of one program header.'],
  ['e_phnum', 56, 58, 'Number of program headers.'],
  ['e_shentsize', 58, 60, 'Size of one section header.'],
  ['e_shnum', 60, 62, 'Number of section headers.'],
  ['e_shstrndx', 62, 64, 'Which section holds the section names.'],
];
const PF = (flags: number) =>
  [
    ['R', 4],
    ['W', 2],
    ['X', 1],
  ]
    .filter(([, bit]) => flags & (bit as number))
    .map(([letter]) => letter)
    .join('');

/** The ELF tone of a section, by what it holds (code, constants, writable data, metadata). */
export function elfSectionTone(name: string, flags: string): string {
  if (name === '.text') return ELF_REGION_COLORS.text;
  if (name === '.rodata') return ELF_REGION_COLORS.rodata;
  if (name === '.eh_frame') return ELF_REGION_COLORS.ehFrame;
  // SHF_ALLOC (0x2) clear: not loaded, metadata for tools (.comment, .riscv.attributes, .shstrtab).
  if ((BigInt(flags) & BigInt(2)) === BigInt(0)) return ELF_REGION_COLORS.meta;
  if ((BigInt(flags) & BigInt(1)) !== BigInt(0)) return ELF_REGION_COLORS.data;
  return ELF_REGION_COLORS.rodata;
}

/**
 * Every byte of a RISC-V ELF64 code cell: the 19 header fields, each program header, every
 * section with file bytes, the unassigned (padding) ranges the API reports, and the section
 * header table. Throws unless they cover the file exactly once.
 */
export function elfParts(cell: Cell): CellPart[] {
  const elf = cell.elf;
  if (!elf) return [];
  const bytes = hexBytes(cell.data ?? '0x', 'CellData');
  const { header } = elf;
  if (header.class !== 'ELF64' || header.headerSize !== 64)
    throw new Error(`Code cell ${cell.txHash}:${cell.outputIndex} is ${header.class}, not ELF64`);
  const headerValue: Record<string, string> = {
    'e_ident · magic': '7f 45 4c 46 = "\\x7fELF"',
    'e_ident · class': header.class,
    'e_ident · data': `${header.endian} endian`,
    e_type: elfTypeLabel(Number.parseInt(bytes.slice(16, 18).reverse().join(''), 16)),
    e_machine: elfMachineLabel(header.machine),
    e_entry: `0x${BigInt(header.entry).toString(16)}`,
    e_phoff: BigInt(header.programHeaderOffset).toLocaleString('en-US'),
    e_shoff: BigInt(header.sectionHeaderOffset).toLocaleString('en-US'),
    e_flags: `0x${header.flags.toString(16)}`,
    e_ehsize: String(header.headerSize),
    e_phentsize: String(header.programHeaderEntrySize),
    e_phnum: String(header.programHeaderCount),
    e_shentsize: String(header.sectionHeaderEntrySize),
    e_shnum: String(header.sectionHeaderCount),
    e_shstrndx: String(header.sectionNameIndex),
  };
  const parts: CellPart[] = ELF64_HEADER.map(([label, start, end, meaning], i) => ({
    id: `elf:eh${i}`,
    block: 'data',
    group: 'data',
    label,
    start,
    end,
    occupied: true,
    tone: ELF_REGION_COLORS.header,
    type: 'ELF header',
    value: headerValue[label] ?? `0x${bytes.slice(start, end).join('')}`,
    meaning,
    indexed: ['e_ident · magic', 'e_machine', 'e_entry', 'e_flags'].includes(label),
  }));
  const phoff = Number(header.programHeaderOffset);
  for (const ph of elf.programHeaders) {
    const start = phoff + ph.index * header.programHeaderEntrySize;
    const size = BigInt(ph.fileSize);
    parts.push({
      id: `elf:ph${ph.index}`,
      block: 'data',
      group: 'data',
      label: `program header ${ph.index} · ${elfProgramTypeLabel(ph.kind)}`,
      start,
      end: start + header.programHeaderEntrySize,
      occupied: true,
      tone: ELF_REGION_COLORS.programHeaders,
      type: 'Elf64_Phdr',
      value: `${PF(ph.flags)} · file [${BigInt(ph.fileOffset).toLocaleString('en-US')}..${(BigInt(ph.fileOffset) + size).toLocaleString('en-US')}) → 0x${BigInt(ph.virtualAddress).toString(16)}${ph.memorySize !== ph.fileSize ? ` · memsz ${BigInt(ph.memorySize).toLocaleString('en-US')}` : ''}`,
      meaning:
        ph.kind === 1
          ? `Loads file bytes into memory with permissions ${PF(ph.flags) || 'none'}.`
          : 'Describes how CKB-VM loads the program.',
      indexed: ph.kind === 1 && (ph.flags & 1) !== 0,
    });
  }
  for (const section of elf.sections) {
    if (section.fileOffset === null || section.fileSize === null || section.fileSize === '0')
      continue;
    const start = Number(section.fileOffset);
    const size = Number(section.fileSize);
    parts.push({
      id: `elf:s${section.index}`,
      block: 'data',
      group: 'data',
      label: section.name || `section ${section.index}`,
      start,
      end: start + size,
      occupied: true,
      tone: elfSectionTone(section.name, section.flags),
      type: 'section',
      value:
        section.name === '.comment' && elf.comment
          ? elf.comment
          : section.name === '.riscv.attributes' && elf.riscvAttributes
            ? elf.riscvAttributes
            : `${size.toLocaleString('en-US')} B${BigInt(section.address) ? ` → 0x${BigInt(section.address).toString(16)}` : ''}`,
      meaning:
        (BigInt(section.flags) & BigInt(2)) === BigInt(0)
          ? 'Not loaded into memory: metadata for tools.'
          : 'Loaded into memory when the program runs.',
      indexed: true,
    });
  }
  for (const gap of elf.unassignedFileRanges)
    parts.push({
      id: `elf:pad${gap.start}`,
      block: 'data',
      group: 'data',
      label: 'alignment padding',
      start: gap.start,
      end: gap.end,
      occupied: true,
      tone: ELF_REGION_COLORS.padding,
      type: null,
      value: bytes.slice(gap.start, Math.min(gap.end, gap.start + 8)).join(' '),
      meaning: 'Bytes no header or section claims. Still occupied: every data byte counts.',
      indexed: true,
    });
  const shoff = Number(header.sectionHeaderOffset);
  const shEnd = shoff + header.sectionHeaderCount * header.sectionHeaderEntrySize;
  if (shEnd > shoff)
    parts.push({
      id: 'elf:shdr',
      block: 'data',
      group: 'data',
      label: 'section header table',
      start: shoff,
      end: shEnd,
      occupied: true,
      tone: ELF_REGION_COLORS.sectionHeaders,
      type: `Elf64_Shdr × ${header.sectionHeaderCount}`,
      value: `${header.sectionHeaderCount} × ${header.sectionHeaderEntrySize} B`,
      meaning: 'Describes the sections. The machine never reads it: it is for tools.',
      indexed: true,
    });
  parts.sort((a, b) => a.start - b.start);
  let end = 0;
  for (const part of parts) {
    if (part.start !== end)
      throw new Error(
        `Code cell ${cell.txHash}:${cell.outputIndex}: ELF parts ${part.start < end ? 'overlap' : 'leave a gap'} at byte ${Math.min(part.start, end)} (${part.label})`
      );
    end = part.end;
  }
  if (end !== bytes.length)
    throw new Error(
      `Code cell ${cell.txHash}:${cell.outputIndex}: ELF parts end at ${end}, data has ${bytes.length} bytes`
    );
  return parts;
}

/** The data block's parts: the ELF layout for code, else the API's deterministic decode. */
export function cellDataParts(cell: Cell): CellPart[] {
  if (cell.elf) return elfParts(cell);
  const length = hexBytes(cell.data ?? '0x', 'CellData').length;
  return length ? decodedDataParts(cell, length) : [];
}

/** The binary map of an ELF code cell: header, program headers, each section, padding, table. */
export function elfRegions(
  parts: readonly CellPart[]
): { id: string; label: string; start: number; end: number; tone: string; partIds: string[] }[] {
  const regions: {
    id: string;
    label: string;
    start: number;
    end: number;
    tone: string;
    partIds: string[];
  }[] = [];
  for (const part of parts) {
    const aggregate = part.id.startsWith('elf:eh')
      ? { id: 'elf:header', label: 'ELF header' }
      : part.id.startsWith('elf:ph')
        ? { id: 'elf:program-headers', label: 'program headers' }
        : null;
    const last = regions.at(-1);
    if (aggregate && last?.id === aggregate.id) {
      last.end = part.end;
      last.partIds.push(part.id);
      continue;
    }
    regions.push({
      id: aggregate?.id ?? part.id,
      label: aggregate?.label ?? part.label,
      start: part.start,
      end: part.end,
      tone: part.tone,
      partIds: [part.id],
    });
  }
  return regions;
}
