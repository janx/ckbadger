'use client';
import { useState, type CSSProperties, type ReactNode } from 'react';
import { hexBytes, validateByteRanges, type ByteRange } from '@/lib/hex-bytes';
import {
  bytePieces,
  META_CHAR_PX,
  SCALE_DEFAULTS,
  scaleLayout,
  scaleRulerTicks,
} from '@/lib/byte-map';
import { useElementWidth } from '@/hooks/useElementWidth';
import { cn } from '@/lib/utils';
export type HexField = ByteRange & { color?: string };

export interface HexMapProps {
  hex: string;
  fields: readonly HexField[];
  subject: string;
  /**
   * The field under the pointer or focus (null when it leaves). With `layout="scale"` a run of
   * bytes no field describes reports `gap:<start>-<end>`.
   */
  onFieldHover?: (field: string | null) => void;
  /** Absolute position of the first byte when showing a page of a payload. */
  offsetBase?: number;
  /**
   * Lane E (C5), additive: `bytes` (the default) draws every byte; `scale` draws the payload to
   * scale, one part per field with its label inside, the bytes no field describes as quiet parts,
   * and an offsets ruler beneath — for payloads too long to draw byte by byte.
   */
  layout?: 'bytes' | 'scale';
  /** `scale` only: the part shown as selected (the caller's hovered or pinned field). */
  activeField?: string | null;
  /** `scale` only: a tap on a part (same ids as `onFieldHover`), for a pinned readout. */
  onFieldSelect?: (field: string) => void;
}

/** A field's bytes, or the bytes no field describes, as an id the hover reports (`scale`). */
export const gapId = (start: number, end: number) => `gap:${start}-${end}`;

export function HexMap(props: HexMapProps) {
  return props.layout === 'scale' ? <HexScaleMap {...props} /> : <HexBytesMap {...props} />;
}

function HexBytesMap({ hex, fields, subject, onFieldHover, offsetBase = 0 }: HexMapProps) {
  const bytes = hexBytes(hex, subject);
  validateByteRanges(fields, bytes.length, subject);
  if (
    !Number.isSafeInteger(offsetBase) ||
    offsetBase < 0 ||
    !Number.isSafeInteger(offsetBase + bytes.length)
  )
    throw new Error(`${subject}: invalid byte-map base offset ${offsetBase}`);
  const [active, setActive] = useState<string | null>(null);
  const select = (id: string | null) => {
    setActive(id);
    onFieldHover?.(id);
  };
  const selected = fields.find((f) => f.id === active);
  return (
    <div aria-label={`${subject} byte map`} className="min-w-0 space-y-3">
      <div className="flex flex-wrap gap-1">
        {bytes.map((byte, i) => {
          const owners = fields.filter((f) => i >= f.offset && i < f.offset + f.length);
          const highlighted = active !== null && owners.some((f) => f.id === active);
          return (
            <button
              type="button"
              key={i}
              data-byte-offset={offsetBase + i}
              data-highlighted={highlighted}
              title={`Byte ${offsetBase + i}: 0x${byte}${owners.length ? ` · ${owners.map((f) => f.label).join(', ')}` : ''}`}
              onMouseEnter={() => select(owners[0]?.id ?? null)}
              onMouseLeave={() => select(null)}
              onFocus={() => select(owners[0]?.id ?? null)}
              onBlur={() => select(null)}
              className={cn(
                'text-small min-w-[2.5ch] rounded-sm px-0.5 py-1 tabular-nums',
                highlighted ? 'bg-base-elevated text-text-bright' : 'text-text-dim'
              )}
              style={{ color: owners[0]?.color }}
            >
              {byte}
            </button>
          );
        })}
      </div>
      <div className="flex flex-wrap gap-3">
        {fields.map((f) => (
          <button
            type="button"
            key={f.id}
            onMouseEnter={() => select(f.id)}
            onMouseLeave={() => select(null)}
            onFocus={() => select(f.id)}
            onBlur={() => select(null)}
            className={cn(
              'text-meta text-left',
              active === f.id ? 'text-text-bright' : 'text-text-dim'
            )}
          >
            {f.label}{' '}
            <span className="text-text-dim">
              bytes {offsetBase + f.offset}–{offsetBase + f.offset + f.length - 1}
            </span>
          </button>
        ))}
      </div>
      <p aria-live="polite" className="text-text text-small min-h-5">
        {selected ? selected.description : `${bytes.length} bytes · inspect a field or byte`}
      </p>
    </div>
  );
}

const fmt = (n: number) => n.toLocaleString('en-US');
/** The ink of a field without its own colour (the cell page's data colour). */
const FIELD_INK = '#6fc3d9';

/**
 * `layout="scale"`: the payload cut where its fields start and end. A run read by several fields
 * (overlapping evidence) belongs to the narrowest of them; its title names them all.
 */
function HexScaleMap({
  hex,
  fields,
  subject,
  onFieldHover,
  offsetBase = 0,
  activeField,
  onFieldSelect,
}: HexMapProps) {
  const bytes = hexBytes(hex, subject);
  validateByteRanges(fields, bytes.length, subject, { allowEmpty: true });
  if (offsetBase !== 0)
    throw new Error(`${subject}: a to-scale byte map draws the whole payload from byte 0`);
  const byId = new Map(fields.map((f) => [f.id, f]));
  const pieces = bytePieces(
    bytes.length,
    fields.map((f) => ({ id: f.id, start: f.offset, end: f.offset + f.length })),
    subject
  );
  const segments: ScaleSegment[] = pieces.map((piece) => {
    const n = piece.end - piece.start;
    const span = `[${fmt(piece.start)}..${fmt(piece.end)}) · ${fmt(n)} B`;
    if (!piece.ids.length)
      return {
        key: gapId(piece.start, piece.end),
        target: gapId(piece.start, piece.end),
        start: piece.start,
        end: piece.end,
        label: fmt(n),
        short: fmt(n),
        color: '#4b5266',
        quiet: true,
        title: `${span} · no field describes ${n === 1 ? 'this byte' : 'these bytes'}`,
      };
    const owners = piece.ids.map((id) => byId.get(id)!);
    const owner = owners.reduce((a, b) => (b.length < a.length ? b : a));
    return {
      key: `${owner.id}@${piece.start}`,
      target: owner.id,
      start: piece.start,
      end: piece.end,
      label: n ? `${owner.label} ${fmt(n)}` : '',
      short: n ? fmt(n) : undefined,
      color: owner.color ?? FIELD_INK,
      title: `${owners.map((f) => f.label).join(', ')} · ${span}`,
    };
  });
  return (
    <ByteScaleMap
      label={subject}
      total={bytes.length}
      segments={segments}
      active={activeField ?? null}
      onHover={onFieldHover}
      onSelect={onFieldSelect}
    />
  );
}

/** The padding either side of a label inside a part. */
const LABEL_PAD_PX = 4;

/** One part of a to-scale byte map, `[start, end)`; an empty part is drawn as a sliver. */
export interface ScaleSegment {
  key: string;
  start: number;
  end: number;
  /** Written inside the part when it fits. */
  label: string;
  /** Written instead when the label does not fit (a byte count). */
  short?: string;
  /** Top rule, tint and label ink. */
  color: string;
  /** Bytes no field describes: a quiet tint and ink. */
  quiet?: boolean;
  title: string;
  /** What a hover or tap on the part reports; omitted, the part is not interactive. */
  target?: string;
  /** The narrowest the part is drawn (default: room for its short label, else the floor). */
  minWidth?: number;
}

export interface ByteScaleMapProps {
  /** Names the map (`<label> byte map`) and its ruler (`<label> offsets`). */
  label: string;
  total: number;
  segments: readonly ScaleSegment[];
  /** The target drawn as selected. */
  active?: string | null;
  onHover?: (target: string | null) => void;
  onSelect?: (target: string) => void;
  /** A line under the ruler: what the map cannot draw (overlapping ranges). */
  note?: ReactNode;
}

function tint(hex: string, alpha: number): string {
  const h = hex.replace('#', '');
  return `rgba(${Number.parseInt(h.slice(0, 2), 16)},${Number.parseInt(h.slice(2, 4), 16)},${Number.parseInt(h.slice(4, 6), 16)},${alpha})`;
}

/**
 * Bytes drawn to scale (consistency C5 / C6, the boards' "byte by byte" maps): a 22 px row of parts,
 * each as wide as its bytes (never narrower than 3 px; an empty field a 4 px sliver), its name
 * inside where it fits, then an offsets ruler with a tick at every boundary. Parts with a `target`
 * are buttons: hover and focus report it, a tap selects it. Before the box is measured (and in
 * tests without a width) the parts flex by bytes and the ruler waits for a width.
 */
export function ByteScaleMap({
  label,
  total,
  segments,
  active = null,
  onHover,
  onSelect,
  note,
}: ByteScaleMapProps) {
  let cursor = 0;
  for (const s of segments) {
    if (s.start !== cursor || s.end < s.start)
      throw new Error(
        `${label}: byte map part ${s.key} [${s.start}, ${s.end}) breaks at ${cursor}`
      );
    cursor = s.end;
  }
  if (cursor !== total)
    throw new Error(`${label}: byte map parts end at ${cursor}, not ${total} bytes`);
  const [ref, width] = useElementWidth<HTMLDivElement>();
  // Many parts (a 72-gene DNA ruler) share 1 px gaps, so the gaps do not eat a phone column.
  const options = { ...SCALE_DEFAULTS, gap: segments.length > 24 ? 1 : SCALE_DEFAULTS.gap };
  const room = (text: string) => text.length * META_CHAR_PX + 2 * LABEL_PAD_PX;
  const fits = (text: string, w: number) => room(text) <= w;
  // Each part keeps room for its byte count (its short label), as the cell page's map does.
  const layout =
    width === null
      ? null
      : scaleLayout(
          width,
          segments.map((s) => ({
            start: s.start,
            end: s.end,
            min: s.minWidth ?? (s.short ? room(s.short) : undefined),
          })),
          options
        );
  const ticks =
    layout && width !== null
      ? scaleRulerTicks(layout, [...segments.map((s) => s.start), total], width, fmt)
      : null;
  return (
    <div ref={ref} className="min-w-0">
      <div
        role="group"
        aria-label={`${label} byte map`}
        className="flex h-[22px] min-w-0"
        style={{ gap: options.gap }}
        onMouseLeave={onHover ? () => onHover(null) : undefined}
      >
        {segments.map((s, i) => {
          const n = s.end - s.start;
          const w = layout?.[i].w ?? null;
          const text =
            w === null
              ? s.label
              : fits(s.label, w)
                ? s.label
                : s.short && fits(s.short, w)
                  ? s.short
                  : '';
          const on = s.target !== undefined && active === s.target;
          const style: CSSProperties = {
            ...(w !== null
              ? { width: w, flex: 'none' }
              : n === 0
                ? { flex: `0 0 ${SCALE_DEFAULTS.sliver}px` }
                : { flex: `${n} 0 0`, minWidth: SCALE_DEFAULTS.floor }),
            backgroundColor: s.quiet
              ? tint('#dee2ec', on ? 0.1 : 0.04)
              : tint(s.color, on ? 0.28 : 0.12),
            borderTop: `2px solid ${s.color}`,
            color: s.quiet ? '#7c849c' : s.color,
            // padding only around a label: an empty 3 px part must stay 3 px (border-box)
            paddingLeft: text ? LABEL_PAD_PX : 0,
            paddingRight: text ? LABEL_PAD_PX : 0,
          };
          const className = cn(
            'text-meta flex min-w-0 items-center justify-center overflow-hidden whitespace-nowrap rounded-sm',
            on && 'outline outline-1 outline-[#a0a8be]'
          );
          return s.target !== undefined && (onHover || onSelect) ? (
            <button
              type="button"
              key={s.key}
              data-segment={s.key}
              data-target={s.target}
              data-active={on}
              aria-pressed={on}
              aria-label={s.title}
              title={s.title}
              onMouseEnter={onHover ? () => onHover(s.target!) : undefined}
              onFocus={onHover ? () => onHover(s.target!) : undefined}
              onBlur={onHover ? () => onHover(null) : undefined}
              onClick={onSelect ? () => onSelect(s.target!) : undefined}
              className={className}
              style={style}
            >
              {text}
            </button>
          ) : (
            <span
              key={s.key}
              data-segment={s.key}
              title={s.title}
              className={className}
              style={style}
            >
              {text}
            </span>
          );
        })}
      </div>
      {ticks && width !== null && (
        <svg
          role="img"
          aria-label={`${label} offsets: ${ticks.map((t) => fmt(t.value)).join(', ')}`}
          width={width}
          height={18}
          className="mt-1 block"
        >
          {ticks.map((t, i) => (
            <g key={i}>
              <line
                x1={Math.min(Math.max(t.x, 0.5), width - 0.5)}
                x2={Math.min(Math.max(t.x, 0.5), width - 0.5)}
                y1={0}
                y2={4}
                stroke="#343c50"
              />
              {t.label && (
                <text x={t.x} y={15} textAnchor={t.anchor} fill="#4b5266" className="text-meta">
                  {t.label}
                </text>
              )}
            </g>
          ))}
        </svg>
      )}
      {note && <div className="text-text-dim text-meta mt-1">{note}</div>}
    </div>
  );
}
