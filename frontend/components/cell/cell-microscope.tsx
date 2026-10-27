'use client';
import { useState } from 'react';
import { HexMap, type HexField } from '@/components/ui/hex-map';
import { hexBytes } from '@/lib/hex-bytes';
import type { Cell } from '@/lib/api';

const COLORS = {
  capacity: '#dee2ec',
  lock: '#7aa7d6',
  type: '#b5c77a',
  data: '#6fc3d9',
  framing: '#a0a8be',
};

function colorFor(label: string, occupied: boolean): string {
  if (!occupied) return COLORS.framing;
  if (label === 'capacity') return COLORS.capacity;
  if (label.startsWith('lock.')) return COLORS.lock;
  if (label.startsWith('type.')) return COLORS.type;
  return COLORS.framing;
}

export function moleculeFields(cell: Cell): HexField[] {
  const molecule = cell.cellOutputMolecule;
  if (!molecule) return [];
  const length = hexBytes(molecule.bytes, 'CellOutput').length;
  let end = 0;
  const fields = molecule.fields.map((field) => {
    if (field.start !== end || field.end <= field.start || field.end > length)
      throw new Error(`CellOutput field map: ${field.label} does not partition ${length} bytes`);
    end = field.end;
    return {
      id: field.label,
      label: field.label.replaceAll('_', ' '),
      offset: field.start,
      length: field.end - field.start,
      description: `${field.label}: bytes ${field.start}–${field.end - 1}. ${field.occupied ? 'Counts toward occupied capacity.' : 'Molecule framing; does not count toward occupied capacity.'}`,
      color: colorFor(field.label, field.occupied),
    };
  });
  if (end !== length) throw new Error(`CellOutput field map ends at ${end}, file has ${length}`);
  return fields;
}

export function dataFields(cell: Cell): HexField[] {
  if (!cell.data) return [];
  const length = hexBytes(cell.data, 'CellData').length;
  if (cell.elf) {
    type Range = { start: number; end: number; label: string; priority: number };
    const ranges: Range[] = [
      { start: 0, end: cell.elf.header.headerSize, label: 'ELF header', priority: 4 },
      {
        start: Number(cell.elf.header.programHeaderOffset),
        end:
          Number(cell.elf.header.programHeaderOffset) +
          cell.elf.header.programHeaderCount * cell.elf.header.programHeaderEntrySize,
        label: 'Program header table',
        priority: 4,
      },
      {
        start: Number(cell.elf.header.sectionHeaderOffset),
        end:
          Number(cell.elf.header.sectionHeaderOffset) +
          cell.elf.header.sectionHeaderCount * cell.elf.header.sectionHeaderEntrySize,
        label: 'Section header table',
        priority: 4,
      },
      ...cell.elf.programHeaders.map((item) => ({
        start: Number(item.fileOffset),
        end: Number(BigInt(item.fileOffset) + BigInt(item.fileSize)),
        label: `Program segment #${item.index}`,
        priority: 1,
      })),
      ...cell.elf.sections
        .filter((item) => item.fileOffset !== null && item.fileSize !== null)
        .map((item) => ({
          start: Number(item.fileOffset),
          end: Number(BigInt(item.fileOffset!) + BigInt(item.fileSize!)),
          label: item.name || `Section #${item.index}`,
          priority: 3,
        })),
    ].filter((range) => range.end > range.start);
    for (const range of ranges)
      if (
        !Number.isSafeInteger(range.start) ||
        !Number.isSafeInteger(range.end) ||
        range.start < 0 ||
        range.end > length
      )
        throw new Error(`ELF file range ${range.label} exceeds ${length} bytes`);
    const boundaries = [
      ...new Set([0, length, ...ranges.flatMap((range) => [range.start, range.end])]),
    ].sort((a, b) => a - b);
    const fields: HexField[] = [];
    for (let index = 0; index < boundaries.length - 1; index++) {
      const start = boundaries[index];
      const end = boundaries[index + 1];
      const owner = ranges
        .filter((range) => range.start <= start && range.end >= end)
        .sort((a, b) => b.priority - a.priority)[0];
      const label = owner?.label ?? 'Unassigned file bytes';
      const previous = fields.at(-1);
      if (previous?.label === label && previous.offset + previous.length === start) {
        previous.length += end - start;
      } else {
        fields.push({
          id: `${label}-${start}`,
          label,
          offset: start,
          length: end - start,
          description: `${label}: file bytes ${start}–${end - 1}. ${owner ? 'Declared ELF range.' : 'Purpose unspecified.'}`,
          color: COLORS.data,
        });
      }
    }
    return fields;
  }
  const segments = [...(cell.dataAnalysis?.deterministic?.segments ?? [])].sort(
    (a, b) => a.start - b.start || a.end - b.end
  );
  const fields: HexField[] = [];
  let end = 0;
  for (const segment of segments) {
    if (segment.start < end || segment.end <= segment.start || segment.end > length)
      throw new Error(`CellData segment ${segment.label} overlaps or exceeds ${length} bytes`);
    if (segment.start > end)
      fields.push({
        id: `unclassified-${end}`,
        label: 'Unclassified bytes',
        offset: end,
        length: segment.start - end,
        description: 'Exact bytes with no deterministic protocol label.',
        color: COLORS.framing,
      });
    fields.push({
      id: `${segment.label}-${segment.start}`,
      label: segment.label.replaceAll('_', ' '),
      offset: segment.start,
      length: segment.end - segment.start,
      description: `${segment.meaning}: ${segment.humanValue}`,
      color: COLORS.data,
    });
    end = segment.end;
  }
  if (end < length)
    fields.push({
      id: `unclassified-${end}`,
      label: 'Unclassified bytes',
      offset: end,
      length: length - end,
      description: 'Exact bytes with no deterministic protocol label.',
      color: COLORS.framing,
    });
  return fields;
}

export function CellMicroscope({ cell }: { cell: Cell }) {
  const [view, setView] = useState<'output' | 'data'>('output');
  const [page, setPage] = useState(0);
  const molecule = cell.cellOutputMolecule;
  const active = view === 'output' && molecule ? 'output' : 'data';
  const bytes = active === 'output' ? molecule!.bytes : (cell.data ?? '0x');
  const fields = active === 'output' ? moleculeFields(cell) : dataFields(cell);
  const length = hexBytes(bytes, active).length;
  const pageCount = Math.max(1, Math.ceil(length / 512));
  const currentPage = Math.min(page, pageCount - 1);
  const offset = currentPage * 512;
  const end = Math.min(length, offset + 512);
  const displayed = `0x${bytes.slice(2 + offset * 2, 2 + end * 2)}`;
  const displayFields = fields
    .filter((field) => field.offset < end && field.offset + field.length > offset)
    .map((field) => ({
      ...field,
      offset: Math.max(field.offset, offset) - offset,
      length: Math.min(field.offset + field.length, end) - Math.max(field.offset, offset),
    }));
  return (
    <section aria-label="Cell byte microscope" className="border-base-border mt-6 border-t pt-5">
      <div className="flex flex-wrap items-baseline justify-between gap-3">
        <h2 className="text-text-bright text-[15px]">The cell, byte by byte</h2>
        <div className="flex gap-2 text-xs">
          <button
            type="button"
            onClick={() => {
              setView('output');
              setPage(0);
            }}
            disabled={!molecule}
            aria-pressed={active === 'output'}
            className="border-base-border rounded border px-2 py-1"
          >
            CellOutput
          </button>
          <button
            type="button"
            onClick={() => {
              setView('data');
              setPage(0);
            }}
            aria-pressed={active === 'data'}
            className="border-base-border rounded border px-2 py-1"
          >
            CellData
          </button>
        </div>
      </div>
      <p className="text-text-dim mt-2 text-xs">
        Molecule framing occupies file bytes; only capacity, script code hashes, hash types and args
        count toward CellOutput occupied capacity.
      </p>
      {active === 'output' && molecule && (
        <div className="text-text-dim my-3 space-y-1 break-all font-mono text-xs">
          <p>
            Lock script hash: <span className="text-[#7aa7d6]">{molecule.lockScriptHash}</span>
          </p>
          {molecule.typeScriptHash && (
            <p>
              Type script hash: <span className="text-[#b5c77a]">{molecule.typeScriptHash}</span>
            </p>
          )}
        </div>
      )}
      <div className="mt-4 overflow-x-auto">
        <HexMap
          hex={displayed}
          fields={displayFields}
          offsetBase={offset}
          subject={active === 'output' ? 'CellOutput' : 'CellData'}
        />
      </div>
      {pageCount > 1 && (
        <div className="text-text-dim mt-3 flex items-center gap-3 text-xs">
          <button
            type="button"
            disabled={currentPage === 0}
            onClick={() => setPage(currentPage - 1)}
          >
            Previous bytes
          </button>
          <span>
            {offset}–{end - 1} of {length} · page {currentPage + 1}/{pageCount}
          </span>
          <button
            type="button"
            disabled={currentPage + 1 === pageCount}
            onClick={() => setPage(currentPage + 1)}
          >
            Next bytes
          </button>
        </div>
      )}
      <div className="text-text-dim mt-4 grid gap-1 text-xs sm:grid-cols-2">
        {fields.map((field) => (
          <button
            type="button"
            key={field.id}
            className="break-all text-left"
            onClick={() => setPage(Math.floor(field.offset / 512))}
          >
            <span style={{ color: field.color }}>{field.label}</span> · {field.offset}–
            {field.offset + field.length - 1}
          </button>
        ))}
      </div>
    </section>
  );
}
