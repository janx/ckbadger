'use client';
import { memo, useMemo, useState, type ReactNode } from 'react';
import { hexBytes } from '@/lib/hex-bytes';
import type { Cell } from '@/lib/api';
import { CELL_COLORS, DATA_COLOR, ELF_REGION_COLORS } from '@/lib/design-tokens';
import {
  cellDataParts,
  cellOutputParts,
  elfRegions,
  type CellGroup,
  type CellPart,
} from '@/lib/cell-parts';
import { cn } from '@/lib/utils';

export { cellDataParts, cellOutputParts } from '@/lib/cell-parts';

const ROW = 24;
/** Data longer than this shows excerpts with "⋯ N bytes … show" rows instead of every byte. */
const FULL_DATA_LIMIT = 1536;
const SHOW_STEP = ROW * 43;
const GROUPS: readonly CellGroup[] = ['capacity', 'lock', 'type', 'data'];

type Active =
  | { kind: 'part'; id: string }
  | { kind: 'group'; group: CellGroup }
  | { kind: 'region'; id: string }
  | null;

function rgba(hex: string, alpha: number): string {
  const h = hex.replace('#', '');
  return `rgba(${Number.parseInt(h.slice(0, 2), 16)},${Number.parseInt(h.slice(2, 4), 16)},${Number.parseInt(h.slice(4, 6), 16)},${alpha})`;
}

const alignDown = (n: number) => n - (n % ROW);
const alignUp = (n: number, max: number) => Math.min(max, n % ROW ? n + ROW - (n % ROW) : n);

/** Which data rows are drawn at first: all of small data; for code, the headers and metadata. */
function initialRanges(cell: Cell, parts: readonly CellPart[], length: number): [number, number][] {
  if (length <= FULL_DATA_LIMIT) return [[0, length]];
  if (cell.elf) {
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
          <span className="[overflow-wrap:anywhere]" style={{ color: color ?? '#dee2ec' }}>
            {value}
          </span>
        </div>
      ))}
    </div>
  );
}

/** One block's hex rows (offset · 24 bytes · ASCII). Independent of hover: highlighting is CSS. */
const HexRows = memo(function HexRows({
  block,
  bytes,
  owner,
  parts,
  from,
  to,
  digits,
}: {
  block: 'output' | 'data';
  bytes: readonly string[];
  owner: readonly string[];
  parts: ReadonlyMap<string, CellPart>;
  from: number;
  to: number;
  /** Offset digits for the whole box, so both blocks' columns line up. */
  digits: 4 | 5;
}) {
  const rows: ReactNode[] = [];
  const width = digits;
  for (let r = alignDown(from); r < to; r += ROW) {
    const hex: ReactNode[] = [];
    const ascii: ReactNode[] = [];
    for (let b = r; b < r + ROW; b++) {
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
          style={{ color: part.occupied ? part.tone : '#343c50' }}
        >
          {ch}
        </span>
      );
    }
    rows.push(
      <div key={`${block}:${r}`} className="flex h-[21px] items-center">
        <span
          className={cn(
            'shrink-0 select-none text-[#4b5266]',
            width === 5 ? 'w-[74px]' : 'w-[62px]'
          )}
        >
          0x{r.toString(16).padStart(width, '0')}
        </span>
        <span className="flex shrink-0 gap-1">{hex}</span>
        <span className="ml-[18px] flex shrink-0 border-l border-[#1a1f30] pl-3.5">{ascii}</span>
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
    `${quoted.map((id) => `[data-cell-bytes] [data-byte][data-part-id=${id}]`).join(',')}{outline:1px solid #a0a8be}`
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
 * byte, field or map part is under the pointer.
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
  const [active, setActive] = useState<Active>(null);
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
      <p role="alert" className="text-rouge text-xs">
        Cell {cell.txHash}:{cell.outputIndex}: its bytes ({outOccupied} occupied CellOutput +{' '}
        {dataBytes.length} data) disagree with its occupied size{' '}
        {breakdown ? `${breakdown.totalBytes} B` : '(missing)'}.
      </p>
    );
  const parts = [...outParts, ...dataParts];
  const regions = cell.elf ? elfRegions(dataParts) : [];
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
  const onHover = (event: React.MouseEvent) => {
    const id = (event.target as HTMLElement).closest<HTMLElement>('[data-part-id]')?.dataset.partId;
    if (id) setActive({ kind: 'part', id });
  };

  const digits: 4 | 5 = Math.max(outBytes.length, dataBytes.length) > 0xffff ? 5 : 4;
  const dataRows: ReactNode[] = [];
  let cursor = 0;
  ranges.forEach(([from, to], i) => {
    if (from > cursor) dataRows.push(gapRow(cursor, from, i));
    dataRows.push(
      <HexRows
        key={`rows:${from}`}
        block="data"
        digits={digits}
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
          'text-text-dim my-1 flex h-[30px] items-center gap-3 border-y border-dashed border-[#1a1f30] text-[11.5px]',
          digits === 5 ? 'pl-[74px]' : 'pl-[62px]'
        )}
      >
        <span>
          ⋯ {fmt(to - from)} bytes [{fmt(from)}..{fmt(to)}): {listed}
        </span>
        <button
          type="button"
          className="ml-auto text-[#6aa9c4] hover:underline"
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
  let detail: ReactNode;
  if (active?.kind === 'part' && byId.has(active.id)) {
    const part = byId.get(active.id)!;
    detail = (
      <div data-detail={part.id}>
        <div className="flex items-baseline justify-between gap-2">
          <span className="text-text-bright text-[13.5px]">{part.label}</span>
          <span className="text-text-dim whitespace-nowrap text-[11px]">
            [{fmt(part.start)}..{fmt(part.end)}) · {fmt(part.end - part.start)} B
          </span>
        </div>
        <p className="mt-1.5 text-[12px] leading-relaxed text-[#a0a8be]">{part.meaning}</p>
        <DetailRows
          rows={[
            [
              'in',
              part.block === 'output'
                ? `outputs[${cell.outputIndex}]`
                : `outputs_data[${cell.outputIndex}]`,
              '#a0a8be',
            ],
            ...(part.type ? [['type', part.type, '#a0a8be'] as [string, ReactNode, string]] : []),
            [
              'raw',
              rawHex(part.block === 'output' ? outBytes : dataBytes, part),
              part.occupied ? part.tone : '#4b5266',
            ],
            ...(part.value !== null ? [['value', part.value] as [string, ReactNode]] : []),
            [
              'occupied',
              part.occupied
                ? `yes, counts toward the ${fmt(breakdown.totalBytes)}`
                : 'no: molecule framing',
              part.occupied ? DATA_COLOR : '#7c849c',
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
          <span className="text-text-bright text-[13.5px]">
            {g}
            {name ? ` · ${name}` : ''}
          </span>
          <span className="text-text-dim whitespace-nowrap text-[11px]">
            {fmt(groupBytes[g])} B occupied
          </span>
        </div>
        <p className="mt-1.5 text-[12px] leading-relaxed text-[#a0a8be]">
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
              [`${g} hash`, `blake2b(bytes ${range[0]}..${range[1]})`, '#7c849c'],
              ['', hash, CELL_COLORS[g]],
            ]}
          />
        )}
        {g === 'data' && cell.dataHash && (
          <DetailRows
            rows={[
              ['data hash', 'blake2b(data)', '#7c849c'],
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
          <span className="text-text-bright text-[13.5px]">{activeRegion.label}</span>
          <span className="text-text-dim whitespace-nowrap text-[11px]">
            [{fmt(activeRegion.start)}..{fmt(activeRegion.end)}) ·{' '}
            {fmt(activeRegion.end - activeRegion.start)} B
          </span>
        </div>
        <p className="mt-1.5 text-[12px] leading-relaxed text-[#a0a8be]">
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
          <span className="text-text-bright text-[13.5px]">Hover a byte, a field or the map</span>
          <span className="text-text-dim whitespace-nowrap text-[11px]">
            {fmt(outBytes.length)} + {fmt(dataBytes.length)} bytes
          </span>
        </div>
        <p className="mt-1.5 text-[12px] leading-relaxed text-[#a0a8be]">
          Each byte belongs to exactly one {cell.elf ? 'part' : 'field'}. Colours follow the four
          parts of the cell; grey bytes are CellOutput framing that the cell does not pay for.
        </p>
        <DetailRows
          rows={[
            [
              'serialized',
              `${fmt(outBytes.length)} B CellOutput + ${fmt(dataBytes.length)} B data`,
              '#a0a8be',
            ],
            [
              'occupied',
              `${fmt(outOccupied)} + ${fmt(dataBytes.length)} = ${fmt(breakdown.totalBytes)} B`,
              DATA_COLOR,
            ],
            [
              'framing',
              `${fmt(outBytes.length - outOccupied)} B of CellOutput headers, not occupied`,
              '#7c849c',
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
      <div className="flex flex-wrap items-baseline justify-between gap-3">
        <h2 className="text-text-bright text-[15px] font-medium">The cell, byte by byte</h2>
        <span className="text-text-dim text-xs">
          {GROUPS.map((g) => fmt(groupBytes[g])).join(' + ')} ={' '}
          <span style={{ color: DATA_COLOR }}>{fmt(breakdown.totalBytes)}</span> occupied bytes ·
          room for {room.toLocaleString('en-US')} more
        </span>
      </div>
      <div
        role="group"
        aria-label="Capacity map"
        className="flex h-[22px] gap-[2px]"
        onMouseLeave={() => setActive(null)}
      >
        {GROUPS.filter((g) => groupBytes[g] > 0).map((g) => (
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
            onMouseEnter={() => setActive({ kind: 'group', group: g })}
            onFocus={() => setActive({ kind: 'group', group: g })}
            onBlur={() => setActive(null)}
            className="flex items-center justify-center rounded-sm text-[10.5px]"
            style={{
              flex: `${groupBytes[g]} 0 0`,
              minWidth: big ? 28 : 8,
              backgroundColor: rgba(CELL_COLORS[g], 0.12),
              borderTop: `2px solid ${CELL_COLORS[g]}`,
              color: CELL_COLORS[g],
            }}
          >
            {groupBytes[g] >= (big ? 1000 : 30) ? `${g} ${fmt(groupBytes[g])}` : groupBytes[g]}
          </button>
        ))}
      </div>
      {regions.length > 0 && (
        <div className="space-y-1.5" onMouseLeave={() => setActive(null)}>
          <div className="text-text-dim flex justify-between text-[11px]">
            <span>the data, to scale: an ELF file</span>
            <span>hover a region</span>
          </div>
          <div role="group" aria-label="ELF binary map" className="flex h-4 gap-px">
            {regions.map((region) => (
              <button
                type="button"
                key={region.id}
                data-region={region.id}
                title={`${region.label} · ${fmt(region.end - region.start)} B`}
                onMouseEnter={() => setActive({ kind: 'region', id: region.id })}
                onFocus={() => setActive({ kind: 'region', id: region.id })}
                onBlur={() => setActive(null)}
                className="flex items-center justify-center overflow-hidden rounded-[1px] text-[10px] text-[#0c0e15]"
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
          <div className="text-text-dim flex flex-wrap gap-x-4 gap-y-1 text-[11px]">
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
                  <span className="text-[#4b5266]">{fmt(region.end - region.start)}</span>
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
                  className="w-full text-[11px] sm:absolute sm:left-0 sm:top-1 sm:w-[74px]"
                  style={{ color: CELL_COLORS[group] }}
                >
                  {group === 'data' && cell.elf ? 'data · ELF' : group}
                </span>
                {own.map((part) => (
                  <button
                    type="button"
                    key={part.id}
                    data-pill={part.id}
                    onMouseEnter={() => setActive({ kind: 'part', id: part.id })}
                    onFocus={() => setActive({ kind: 'part', id: part.id })}
                    onBlur={() => setActive(null)}
                    className={cn(
                      'inline-flex h-6 items-center gap-1.5 rounded border px-2 text-[11.5px]',
                      active?.kind === 'part' && active.id === part.id
                        ? 'border-[#a0a8be]'
                        : 'border-[#1a1f30]',
                      part.occupied ? 'text-text-bright' : 'text-text-dim'
                    )}
                  >
                    <span
                      aria-hidden="true"
                      className="inline-block h-1.5 w-1.5 rounded-full"
                      style={{ backgroundColor: part.occupied ? part.tone : '#343c50' }}
                    />
                    {part.block === 'output' ? part.label.split(' · ').at(-1) : part.label}
                    <span className="text-[10.5px] text-[#4b5266]">
                      [{fmt(part.start)}..{fmt(part.end)})
                    </span>
                  </button>
                ))}
              </div>
            ))}
            {hidden > 0 && (
              <div className="text-text-dim text-[11px] sm:pl-20">
                + {hidden} more ELF header fields and program headers: hover their bytes
              </div>
            )}
          </div>
          <div
            aria-label="Cell bytes"
            data-cell-bytes=""
            data-active={litIds ? litIds.join(' ') : undefined}
            className="overflow-x-auto rounded-md border border-[#1a1f30] px-4 py-3 text-xs tabular-nums"
            onMouseOver={onHover}
            onMouseLeave={() => setActive(null)}
          >
            {molecule && (
              <div aria-label="CellOutput byte map">
                <div className="text-text-dim flex justify-between gap-3 pb-1.5 text-[11px]">
                  <span>outputs[{cell.outputIndex}] · CellOutput · molecule</span>
                  <span>
                    {fmt(outBytes.length)} B · {fmt(outOccupied)} occupied
                  </span>
                </div>
                <HexRows
                  block="output"
                  digits={digits}
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
              className={cn(molecule && 'mt-2.5 border-t border-dashed border-[#1a1f30] pt-3.5')}
            >
              <div className="text-text-dim flex justify-between gap-3 pb-1.5 text-[11px]">
                <span>
                  outputs_data[{cell.outputIndex}]
                  {cell.elf
                    ? ' · ELF64 RISC-V'
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
          </div>
        </div>
        <aside
          aria-label="Field detail"
          aria-live="polite"
          className="min-h-[260px] rounded-md border border-[#1a1f30] px-3.5 py-3 text-xs lg:sticky lg:top-4"
        >
          {detail}
        </aside>
      </div>
    </section>
  );
}
