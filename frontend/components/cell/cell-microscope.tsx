'use client';
import { memo, useMemo, useState, type ReactNode } from 'react';
import { ReadoutRow } from '@/components/ui/readout-row';
import { TerminalPanel, TerminalPanelContent } from '@/components/ui/terminal-panel';
import { PromptHeading } from '@/components/ui/prompt-heading';
import { usePinnedReadout } from '@/hooks/usePinnedReadout';
import { hexBytes } from '@/lib/hex-bytes';
import { fade } from '@/lib/palette';
import type { Cell } from '@/lib/api';
import {
  BOARD_INKS,
  CELL_COLORS,
  DATA_COLOR,
  ELF_REGION_COLORS,
  inkStyle,
} from '@/lib/design-tokens';
import {
  cellDataParts,
  cellOutputParts,
  elfRegions,
  parsedElf,
  type CellGroup,
  type CellPart,
} from '@/lib/cell-parts';
import { cn } from '@/lib/utils';
import { useElementWidth } from '@/hooks/useElementWidth';
import {
  hexRowBytes,
  MAP_PAD_PX,
  mapSegmentMinWidth,
  type HexRowBytes,
} from '@/lib/cell-byte-layout';
import { STICKY_RAIL } from '@/components/layout/sticky-header';

export { cellDataParts, cellOutputParts } from '@/lib/cell-parts';

/** Excerpt ranges align to 24 bytes, which every dump row length (24, 16, 8) divides or fits. */
const ROW = 24;
/** Offsets and sizes in the quiet slate, a text ink read by `text-[color:var(--ink)]`. */
const QUIET = inkStyle(BOARD_INKS.quiet);
/** The capacity map's gap between parts (`gap-[2px]`). */
const MAP_GAP = 2;
/** Data longer than this shows excerpts with "⋯ N bytes … show" rows instead of every byte. */
const FULL_DATA_LIMIT = 1536;
const SHOW_STEP = ROW * 43;
const GROUPS: readonly CellGroup[] = ['capacity', 'lock', 'type', 'data'];

/** What a hover or a tap points at: one part (a byte or a pill), a map group or an ELF region. */
type Target =
  | { kind: 'part'; id: string }
  | { kind: 'group'; group: CellGroup }
  | { kind: 'region'; id: string };
type Active = Target | null;
const targetKey = (t: Target) => (t.kind === 'group' ? `group:${t.group}` : `${t.kind}:${t.id}`);

/** A part's tone (a theme reference) at `alpha`, as a background. */
const rgba = fade;

const alignDown = (n: number) => n - (n % ROW);
const alignUp = (n: number, max: number) => Math.min(max, n % ROW ? n + ROW - (n % ROW) : n);

/** Which data rows are drawn at first: all of small data; for code, the headers and metadata. */
function initialRanges(cell: Cell, parts: readonly CellPart[], length: number): [number, number][] {
  if (length <= FULL_DATA_LIMIT) return [[0, length]];
  if (parsedElf(cell)) {
    const headers = parts.filter((p) => p.id.startsWith('elf:eh') || p.id.startsWith('elf:ph'));
    const headEnd = alignUp(Math.max(...headers.map((p) => p.end)), length);
    const meta = parts.filter(
      (p) => p.id.startsWith('elf:s') && p.tone === ELF_REGION_COLORS.meta && p.start >= headEnd
    );
    if (meta.length) {
      const from = alignDown(Math.min(...meta.map((p) => p.start)));
      const to = alignUp(Math.max(...meta.map((p) => p.end)), length);
      return [
        [0, headEnd],
        [Math.max(from, headEnd), to],
      ];
    }
    return [[0, headEnd]];
  }
  return [
    [0, 768],
    [alignDown(length - 384), length],
  ];
}

function rawHex(bytes: readonly string[], part: CellPart): string {
  const shown = bytes.slice(part.start, Math.min(part.end, part.start + 48)).join('');
  const grouped = shown.match(/.{1,8}/g)?.join(' ') ?? '';
  const n = part.end - part.start;
  return n > 48 ? `${grouped} … (${n.toLocaleString('en-US')} B)` : grouped;
}

function DetailRows({ rows }: { rows: readonly [string, ReactNode, string?][] }) {
  return (
    <div className="mt-2">
      {rows.map(([label, value, color]) => (
        <div key={label} className="grid grid-cols-[78px_minmax(0,1fr)] gap-2 py-0.5">
          <span className="text-text-dim">{label}</span>
          <span
            className="[overflow-wrap:anywhere]"
            style={{ color: color ?? 'var(--c-text-bright)' }}
          >
            {value}
          </span>
        </div>
      ))}
    </div>
  );
}

/** One block's hex rows (offset · bytes · ASCII). Independent of hover: highlighting is CSS. */
const HexRows = memo(function HexRows({
  block,
  bytes,
  owner,
  parts,
  from,
  to,
  digits,
  perRow,
}: {
  block: 'output' | 'data';
  bytes: readonly string[];
  owner: readonly string[];
  parts: ReadonlyMap<string, CellPart>;
  from: number;
  to: number;
  /** Offset digits for the whole box, so both blocks' columns line up. */
  digits: 4 | 5;
  /** Bytes a row: as many whole bytes as the measured box holds. */
  perRow: HexRowBytes;
}) {
  const rows: ReactNode[] = [];
  const width = digits;
  for (let r = from - (from % perRow); r < to; r += perRow) {
    const hex: ReactNode[] = [];
    const ascii: ReactNode[] = [];
    for (let b = r; b < r + perRow; b++) {
      if (b < from || b >= to || b >= bytes.length) {
        hex.push(
          <span key={b} aria-hidden="true" data-pad="" className="invisible rounded-sm px-px">
            00
          </span>
        );
        ascii.push(
          <span key={b} aria-hidden="true" className="invisible">
            .
          </span>
        );
        continue;
      }
      const part = parts.get(owner[b])!;
      const code = Number.parseInt(bytes[b], 16);
      const ch =
        code === 32 ? '\u00a0' : code > 32 && code <= 126 ? String.fromCharCode(code) : '.';
      hex.push(
        <span
          key={b}
          data-part-id={part.id}
          data-byte={`${block}:${b}`}
          title={`${part.label} · byte ${b.toLocaleString('en-US')}`}
          className="rounded-sm px-px"
          style={{
            color: part.tone,
            backgroundColor: part.occupied ? rgba(part.tone, 0.15) : 'transparent',
          }}
        >
          {bytes[b]}
        </span>
      );
      ascii.push(
        <span
          key={b}
          data-part-id={part.id}
          style={{ color: part.occupied ? part.tone : 'var(--c-text-ghost)' }}
        >
          {ch}
        </span>
      );
    }
    rows.push(
      <div key={`${block}:${r}`} className="flex h-[21px] items-center">
        <span
          className={cn(
            'shrink-0 select-none text-[color:var(--ink)]',
            width === 5 ? 'w-[74px]' : 'w-[62px]'
          )}
          style={QUIET}
        >
          0x{r.toString(16).padStart(width, '0')}
        </span>
        <span className="flex shrink-0 gap-1">{hex}</span>
        <span className="border-base-border-subtle ml-[18px] flex shrink-0 border-l pl-3.5">
          {ascii}
        </span>
      </div>
    );
  }
  return <>{rows}</>;
});

/** Light the bytes of `ids` and dim the rest, as one stylesheet rule set (no per-byte render). */
export function highlightCss(ids: readonly string[] | null): string {
  if (!ids) return '';
  const quoted = ids.map((id) => `"${id.replace(/["\\]/g, '\\$&')}"`);
  const own = quoted.map((id) => `[data-cell-bytes] [data-part-id=${id}]`).join(',');
  return (
    `[data-cell-bytes] [data-part-id]{opacity:.25}` +
    `${own}{opacity:1}` +
    `${quoted.map((id) => `[data-cell-bytes] [data-byte][data-part-id=${id}]`).join(',')}{outline:1px solid var(--c-text)}`
  );
}

export interface CellMicroscopeProps {
  cell: Cell;
  /** Registry names of its lock and type scripts, for the map detail. */
  lockName?: string | null;
  typeName?: string | null;
}

/**
 * "The cell, byte by byte" (Cell board v3): one microscope — the capacity map, a field index,
 * one hex box with the CellOutput and its data, and one detail panel that explains whatever
 * byte, field or map part is under the pointer. A tap pins it, with a readout line under the
 * bytes, until a second tap on it or Escape (R6).
 */
export function CellMicroscope({ cell, lockName, typeName }: CellMicroscopeProps) {
  const molecule = cell.cellOutputMolecule;
  const outBytes = useMemo(
    () => (molecule ? hexBytes(molecule.bytes, 'CellOutput') : []),
    [molecule]
  );
  const dataBytes = useMemo(() => hexBytes(cell.data ?? '0x', 'CellData'), [cell.data]);
  const outParts = useMemo(() => cellOutputParts(cell), [cell]);
  const dataParts = useMemo(() => cellDataParts(cell), [cell]);
  const readout = usePinnedReadout<Target>(targetKey);
  const active: Active = readout.active;
  const setActive = readout.hover;
  /** Hover, focus and tap wired to one button (a map part, an ELF region or a field pill). */
  const handlers = (target: Target) => ({
    'aria-pressed': readout.isPinned(target),
    onMouseEnter: () => setActive(target),
    onFocus: () => setActive(target),
    onBlur: () => setActive(null),
    onClick: () => readout.toggle(target),
  });
  const [bytesRef, bytesWidth] = useElementWidth<HTMLDivElement>();
  const [ranges, setRanges] = useState<[number, number][]>(() =>
    initialRanges(cell, dataParts, dataBytes.length)
  );
  const byId = useMemo(
    () => new Map([...outParts, ...dataParts].map((p) => [p.id, p])),
    [outParts, dataParts]
  );
  const outOwner = useMemo(() => {
    const owner: string[] = [];
    for (const p of outParts) for (let b = p.start; b < p.end; b++) owner[b] = p.id;
    return owner;
  }, [outParts]);
  const dataOwner = useMemo(() => {
    const owner: string[] = [];
    for (const p of dataParts) for (let b = p.start; b < p.end; b++) owner[b] = p.id;
    return owner;
  }, [dataParts]);
  const breakdown = cell.commonKnowledgeSizeBreakdown;
  const outOccupied = outParts.filter((p) => p.occupied).reduce((n, p) => n + p.end - p.start, 0);
  if (!breakdown || (molecule && outOccupied + dataBytes.length !== breakdown.totalBytes))
    return (
      <p role="alert" className="text-rouge text-small">
        Cell {cell.txHash}:{cell.outputIndex}: its bytes ({outOccupied} occupied CellOutput +{' '}
        {dataBytes.length} data) disagree with its occupied size{' '}
        {breakdown ? `${breakdown.totalBytes} B` : '(missing)'}.
      </p>
    );
  const parts = [...outParts, ...dataParts];
  const elf = parsedElf(cell);
  const regions = elf ? elfRegions(dataParts) : [];
  const activeRegion = active?.kind === 'region' ? regions.find((r) => r.id === active.id) : null;
  const litIds =
    active === null
      ? null
      : active.kind === 'part'
        ? [active.id]
        : active.kind === 'group'
          ? parts.filter((p) => p.group === active.group).map((p) => p.id)
          : (activeRegion?.partIds ?? []);
  const groupBytes: Record<CellGroup, number> = {
    capacity: breakdown.capacityFieldBytes,
    lock: breakdown.lockScriptBytes,
    type: breakdown.typeScriptBytes,
    data: breakdown.dataBytes,
  };
  const capacity = BigInt(cell.capacity);
  const occupied = BigInt(breakdown.totalBytes) * BigInt(100000000);
  if (occupied > capacity)
    throw new Error(`Cell ${cell.txHash}:${cell.outputIndex} occupies more than its capacity`);
  const room = (capacity - occupied) / BigInt(100000000);
  const big = breakdown.totalBytes >= 1000;
  const fmt = (n: number) => n.toLocaleString('en-US');
  // The map's parts: every non-empty group, named in full once it is large enough to hold it.
  const mapSegments = GROUPS.filter((g) => groupBytes[g] > 0).map((group) => ({
    group,
    bytes: groupBytes[group],
    label:
      groupBytes[group] >= (big ? 1000 : 30)
        ? `${group} ${fmt(groupBytes[group])}`
        : fmt(groupBytes[group]),
  }));

  const partAt = (event: React.MouseEvent): Target | null => {
    const id = (event.target as HTMLElement).closest<HTMLElement>('[data-part-id]')?.dataset.partId;
    return id ? { kind: 'part', id } : null;
  };
  const onHover = (event: React.MouseEvent) => {
    const part = partAt(event);
    if (part) setActive(part);
  };
  const onTap = (event: React.MouseEvent) => {
    const part = partAt(event);
    if (part) readout.toggle(part);
  };

  const digits: 4 | 5 = Math.max(outBytes.length, dataBytes.length) > 0xffff ? 5 : 4;
  // Until the box is measured (before the first paint) rows are the desktop's 24 bytes.
  const perRow: HexRowBytes = bytesWidth === null ? 24 : hexRowBytes(bytesWidth, digits);
  const dataRows: ReactNode[] = [];
  let cursor = 0;
  ranges.forEach(([from, to], i) => {
    if (from > cursor) dataRows.push(gapRow(cursor, from, i));
    dataRows.push(
      <HexRows
        key={`rows:${from}`}
        block="data"
        digits={digits}
        perRow={perRow}
        bytes={dataBytes}
        owner={dataOwner}
        parts={byId}
        from={from}
        to={to}
      />
    );
    cursor = to;
  });
  if (cursor < dataBytes.length) dataRows.push(gapRow(cursor, dataBytes.length, ranges.length));
  function gapRow(from: number, to: number, index: number) {
    const names = [
      ...new Set(dataParts.filter((p) => p.start < to && p.end > from).map((p) => p.label)),
    ];
    const listed =
      names.length > 6
        ? `${names.slice(0, 5).join(', ')} and ${names.length - 5} more`
        : names.join(', ');
    return (
      <div
        key={`gap:${from}`}
        data-gap={`${from}-${to}`}
        className={cn(
          'text-text-dim text-meta border-base-border-subtle my-1 flex h-[30px] items-center gap-3 border-y border-dashed',
          digits === 5 ? 'pl-[74px]' : 'pl-[62px]'
        )}
      >
        <span>
          ⋯ {fmt(to - from)} bytes [{fmt(from)}..{fmt(to)}): {listed}
        </span>
        <button
          type="button"
          className="ml-auto text-[color:var(--ink)] hover:underline"
          style={inkStyle(BOARD_INKS.link)}
          onClick={() =>
            setRanges((current) => {
              const next = current.map((range) => [...range] as [number, number]);
              const reveal: [number, number] = [from, Math.min(to, from + SHOW_STEP)];
              next.splice(index, 0, reveal);
              next.sort((a, b) => a[0] - b[0]);
              const merged: [number, number][] = [];
              for (const range of next) {
                const last = merged.at(-1);
                if (last && range[0] <= last[1]) last[1] = Math.max(last[1], range[1]);
                else merged.push(range);
              }
              return merged;
            })
          }
        >
          show
        </button>
      </div>
    );
  }

  const groupRange = (group: 'lock' | 'type') => {
    const own = outParts.filter((p) => p.group === group);
    return own.length ? [own[0].start, own.at(-1)!.end] : null;
  };
  const span = (start: number, end: number) =>
    `[${fmt(start)}..${fmt(end)}) · ${fmt(end - start)} B`;
  /** The readout line under the bytes: the hovered or pinned target in one line (R6). */
  const line = (target: Target): string => {
    if (target.kind === 'group') {
      const g = target.group;
      const name = g === 'lock' ? lockName : g === 'type' ? typeName : null;
      return `${g}${name ? ` · ${name}` : ''} · ${fmt(groupBytes[g])} B occupied`;
    }
    if (target.kind === 'region') {
      const region = regions.find((r) => r.id === target.id);
      if (!region)
        throw new Error(`Cell ${cell.txHash}:${cell.outputIndex}: no ELF region ${target.id}`);
      return `${region.label} · ${span(region.start, region.end)}`;
    }
    const part = byId.get(target.id);
    if (!part)
      throw new Error(`Cell ${cell.txHash}:${cell.outputIndex}: no byte part ${target.id}`);
    return `${part.label} · ${span(part.start, part.end)}${part.value !== null ? ` · ${part.value}` : ''}`;
  };
  let detail: ReactNode;
  if (active?.kind === 'part' && byId.has(active.id)) {
    const part = byId.get(active.id)!;
    detail = (
      <div data-detail={part.id}>
        <div className="flex items-baseline justify-between gap-2">
          <span className="text-text-bright text-body">{part.label}</span>
          <span className="text-text-dim text-meta whitespace-nowrap">
            [{fmt(part.start)}..{fmt(part.end)}) · {fmt(part.end - part.start)} B
          </span>
        </div>
        <p className="text-small text-text mt-1.5 leading-relaxed">{part.meaning}</p>
        <DetailRows
          rows={[
            [
              'in',
              part.block === 'output'
                ? `outputs[${cell.outputIndex}]`
                : `outputs_data[${cell.outputIndex}]`,
              'var(--c-text)',
            ],
            ...(part.type
              ? [['type', part.type, 'var(--c-text)'] as [string, ReactNode, string]]
              : []),
            [
              'raw',
              rawHex(part.block === 'output' ? outBytes : dataBytes, part),
              part.occupied ? part.tone : 'var(--c-ink-quiet)',
            ],
            ...(part.value !== null ? [['value', part.value] as [string, ReactNode]] : []),
            [
              'occupied',
              part.occupied
                ? `yes, counts toward the ${fmt(breakdown.totalBytes)}`
                : 'no: molecule framing',
              part.occupied ? DATA_COLOR : 'var(--c-text-dim)',
            ],
          ]}
        />
      </div>
    );
  } else if (active?.kind === 'group') {
    const g = active.group;
    const range = g === 'lock' || g === 'type' ? groupRange(g) : null;
    const hash =
      g === 'lock' ? molecule?.lockScriptHash : g === 'type' ? molecule?.typeScriptHash : null;
    const name = g === 'lock' ? lockName : g === 'type' ? typeName : null;
    detail = (
      <div data-detail={`group:${g}`}>
        <div className="flex items-baseline justify-between gap-2">
          <span className="text-text-bright text-body">
            {g}
            {name ? ` · ${name}` : ''}
          </span>
          <span className="text-text-dim text-meta whitespace-nowrap">
            {fmt(groupBytes[g])} B occupied
          </span>
        </div>
        <p className="text-small text-text mt-1.5 leading-relaxed">
          {g === 'capacity'
            ? 'The capacity field takes 8 bytes of the capacity it states.'
            : g === 'lock'
              ? 'Who can spend the cell. Occupied: code_hash 32 + hash_type 1 + args.'
              : g === 'type'
                ? 'What the cell is and how it may change. Occupied: code_hash 32 + hash_type 1 + args.'
                : 'Every byte of data is occupied, its own framing included.'}
        </p>
        {range && hash && (
          <DetailRows
            rows={[
              [`${g} hash`, `blake2b(bytes ${range[0]}..${range[1]})`, 'var(--c-text-dim)'],
              ['', hash, CELL_COLORS[g]],
            ]}
          />
        )}
        {g === 'data' && cell.dataHash && (
          <DetailRows
            rows={[
              ['data hash', 'blake2b(data)', 'var(--c-text-dim)'],
              ['', cell.dataHash, CELL_COLORS.data],
            ]}
          />
        )}
      </div>
    );
  } else if (activeRegion) {
    detail = (
      <div data-detail={activeRegion.id}>
        <div className="flex items-baseline justify-between gap-2">
          <span className="text-text-bright text-body">{activeRegion.label}</span>
          <span className="text-text-dim text-meta whitespace-nowrap">
            [{fmt(activeRegion.start)}..{fmt(activeRegion.end)}) ·{' '}
            {fmt(activeRegion.end - activeRegion.start)} B
          </span>
        </div>
        <p className="text-small text-text mt-1.5 leading-relaxed">
          {activeRegion.partIds.length > 1
            ? `${activeRegion.partIds.length} parts: ${activeRegion.partIds.map((id) => byId.get(id)!.label).join(' · ')}.`
            : byId.get(activeRegion.partIds[0])!.meaning}
        </p>
      </div>
    );
  } else {
    detail = (
      <div data-detail="default">
        <div className="flex items-baseline justify-between gap-2">
          {/* C10: the readout row under the bytes is the section's one "tap or hover". */}
          <span className="text-text-bright text-body">Where each byte belongs</span>
          <span className="text-text-dim text-meta whitespace-nowrap">
            {fmt(outBytes.length)} + {fmt(dataBytes.length)} bytes
          </span>
        </div>
        <p className="text-small text-text mt-1.5 leading-relaxed">
          Each byte belongs to exactly one {elf ? 'part' : 'field'}. Colours follow the four parts
          of the cell; grey bytes are CellOutput framing that the cell does not pay for.
        </p>
        <DetailRows
          rows={[
            [
              'serialized',
              `${fmt(outBytes.length)} B CellOutput + ${fmt(dataBytes.length)} B data`,
              'var(--c-text)',
            ],
            [
              'occupied',
              `${fmt(outOccupied)} + ${fmt(dataBytes.length)} = ${fmt(breakdown.totalBytes)} B`,
              DATA_COLOR,
            ],
            [
              'framing',
              `${fmt(outBytes.length - outOccupied)} B of CellOutput headers, not occupied`,
              'var(--c-text-dim)',
            ],
          ]}
        />
      </div>
    );
  }

  const indexGroups = GROUPS.map((group) => ({
    group,
    parts: parts.filter((p) => p.group === group && p.indexed),
  })).filter((g) => g.parts.length);
  const hidden = dataParts.filter((p) => !p.indexed).length;

  return (
    <section aria-label="Cell byte microscope" className="min-w-0 space-y-3.5">
      <style>{highlightCss(litIds)}</style>
      {/* Rubbing × Song print, phase 5 (A7): the bytes, run on the sum the map draws (its
          occupied total in the data ink); the room left is the right-prompt note. */}
      <PromptHeading
        command="Bytes"
        args={
          <>
            {GROUPS.map((g) => fmt(groupBytes[g])).join(' + ')} ={' '}
            <span style={{ color: DATA_COLOR }}>{fmt(breakdown.totalBytes)}</span>
          </>
        }
        note={`room for ${room.toLocaleString('en-US')} more`}
      />
      {/* A flex row: basis 0, growth by bytes, each part's minimum holds its label. */}
      <div
        role="group"
        aria-label="Capacity map"
        className="flex h-[22px]"
        style={{ gap: MAP_GAP }}
        onMouseLeave={() => setActive(null)}
      >
        {mapSegments.map(({ group: g, bytes, label }) => (
          <button
            type="button"
            key={g}
            data-map={g}
            data-lit={
              active === null
                ? undefined
                : active.kind === 'group'
                  ? active.group === g
                  : active.kind === 'part'
                    ? byId.get(active.id)?.group === g
                    : g === 'data'
            }
            title={`${g} · ${fmt(groupBytes[g])} bytes`}
            {...handlers({ kind: 'group', group: g })}
            className="text-meta flex items-center justify-center whitespace-nowrap rounded-sm"
            style={{
              flex: `${bytes} 0 0`,
              minWidth: mapSegmentMinWidth(label, big ? 28 : 8),
              paddingInline: MAP_PAD_PX,
              backgroundColor: rgba(CELL_COLORS[g], 0.12),
              borderTop: `2px solid ${CELL_COLORS[g]}`,
              color: CELL_COLORS[g],
            }}
          >
            {label}
          </button>
        ))}
      </div>
      {regions.length > 0 && (
        <div className="space-y-1.5" onMouseLeave={() => setActive(null)}>
          <div className="text-text-dim text-meta flex justify-between">
            <span>the data, to scale: an ELF file</span>
          </div>
          <div role="group" aria-label="ELF binary map" className="flex h-4 gap-px">
            {regions.map((region) => (
              <button
                type="button"
                key={region.id}
                data-region={region.id}
                title={`${region.label} · ${fmt(region.end - region.start)} B`}
                {...handlers({ kind: 'region', id: region.id })}
                className="text-meta text-base-bg flex items-center justify-center overflow-hidden rounded-sm"
                style={{
                  flex: `${region.end - region.start} 0 0`,
                  minWidth: 3,
                  backgroundColor: rgba(region.tone, 0.35),
                  borderTop: `2px solid ${region.tone}`,
                }}
              >
                {(region.end - region.start) * 5 > dataBytes.length
                  ? `${region.label} ${fmt(region.end - region.start)}`
                  : ''}
              </button>
            ))}
          </div>
          <div className="text-text-dim text-meta flex flex-wrap gap-x-4 gap-y-1">
            {regions
              .filter((r) => r.label !== 'alignment padding')
              .map((region) => (
                <span key={region.id} className="inline-flex items-center gap-1.5">
                  <span
                    aria-hidden="true"
                    className="inline-block h-2 w-2 rounded-sm"
                    style={{ backgroundColor: region.tone }}
                  />
                  {region.label}{' '}
                  <span className="text-[color:var(--ink)]" style={QUIET}>
                    {fmt(region.end - region.start)}
                  </span>
                </span>
              ))}
          </div>
        </div>
      )}
      <div className="grid items-start gap-6 pt-2 lg:grid-cols-[minmax(0,846px)_minmax(0,1fr)]">
        <div className="min-w-0 space-y-4">
          <div
            aria-label="Field index"
            className="space-y-1.5"
            onMouseLeave={() => setActive(null)}
          >
            {indexGroups.map(({ group, parts: own }) => (
              <div key={group} className="relative flex flex-wrap items-center gap-[5px] sm:pl-20">
                <span
                  data-index-group={group}
                  className="text-meta w-full sm:absolute sm:left-0 sm:top-1 sm:w-[74px]"
                  style={{ color: CELL_COLORS[group] }}
                >
                  {group === 'data' && elf ? 'data · ELF' : group}
                </span>
                {own.map((part) => (
                  <button
                    type="button"
                    key={part.id}
                    data-pill={part.id}
                    {...handlers({ kind: 'part', id: part.id })}
                    className={cn(
                      'text-meta inline-flex h-6 items-center gap-1.5 rounded border px-2',
                      active?.kind === 'part' && active.id === part.id
                        ? 'border-text'
                        : 'border-base-border-subtle',
                      part.occupied ? 'text-text-bright' : 'text-text-dim'
                    )}
                  >
                    <span
                      aria-hidden="true"
                      className="inline-block h-1.5 w-1.5 rounded-full"
                      style={{ backgroundColor: part.occupied ? part.tone : 'var(--c-text-ghost)' }}
                    />
                    {part.block === 'output' ? part.label.split(' · ').at(-1) : part.label}
                    <span className="text-meta text-[color:var(--ink)]" style={QUIET}>
                      [{fmt(part.start)}..{fmt(part.end)})
                    </span>
                  </button>
                ))}
              </div>
            ))}
            {hidden > 0 && (
              <div className="text-text-dim text-meta sm:pl-20">
                + {hidden} more ELF header fields and program headers, named in the line under their
                bytes
              </div>
            )}
          </div>
          <div ref={bytesRef} className="min-w-0">
            {/* Midnight, continued (move 3): the byte dump is an instrument, in the one frame. */}
            <TerminalPanel
              aria-label="Cell bytes"
              data-cell-bytes=""
              data-active={litIds ? litIds.join(' ') : undefined}
              className="text-small tabular-nums"
              onMouseOver={onHover}
              onClick={onTap}
              onMouseLeave={() => setActive(null)}
            >
              {/* The dump itself is a well inside the glass (material pass v2, move 2). */}
              <TerminalPanelContent padding="none" className="well m-2 overflow-x-auto px-4 py-3">
                {molecule && (
                  <div aria-label="CellOutput byte map">
                    <div className="text-text-dim text-meta flex flex-wrap justify-between gap-x-3 pb-1.5">
                      <span>outputs[{cell.outputIndex}] · CellOutput · molecule</span>
                      <span>
                        {fmt(outBytes.length)} B · {fmt(outOccupied)} occupied
                      </span>
                    </div>
                    <HexRows
                      block="output"
                      digits={digits}
                      perRow={perRow}
                      bytes={outBytes}
                      owner={outOwner}
                      parts={byId}
                      from={0}
                      to={outBytes.length}
                    />
                  </div>
                )}
                <div
                  aria-label="CellData byte map"
                  className={cn(
                    molecule && 'border-base-border-subtle mt-2.5 border-t border-dashed pt-3.5'
                  )}
                >
                  <div className="text-text-dim text-meta flex flex-wrap justify-between gap-x-3 pb-1.5">
                    <span>
                      outputs_data[{cell.outputIndex}]
                      {elf
                        ? ' · ELF64 RISC-V'
                        : cell.elf?.status === 'unparsed'
                          ? ` · an ELF file that does not parse: ${cell.elf.reason}`
                          : typeName
                            ? ` · decoded by its type script, ${typeName}`
                            : ''}
                    </span>
                    <span>
                      {fmt(dataBytes.length)} B · all occupied
                      {ranges.length > 1 || (ranges[0] && ranges[0][1] < dataBytes.length)
                        ? ' · excerpts shown'
                        : ''}
                    </span>
                  </div>
                  {dataBytes.length ? (
                    dataRows
                  ) : (
                    <p className="text-text-dim py-1">No data: the cell carries 0 bytes.</p>
                  )}
                </div>
              </TerminalPanelContent>
            </TerminalPanel>
          </div>
          <ReadoutRow label="Byte readout" pinned={readout.showingPin}>
            {active === null ? null : line(active)}
          </ReadoutRow>
        </div>
        <aside
          aria-label="Field detail"
          aria-live="polite"
          className={cn(
            'text-small border-base-border-subtle min-h-[260px] rounded border px-3.5 py-3',
            STICKY_RAIL
          )}
        >
          {detail}
        </aside>
      </div>
    </section>
  );
}
