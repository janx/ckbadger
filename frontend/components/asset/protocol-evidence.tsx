'use client';

import { useState, type ReactNode } from 'react';
import { HexMap, gapId, type HexField } from '@/components/ui/hex-map';
import { TintedJson } from '@/components/ui/tinted-json';
import { TapOrHoverHint } from '@/components/ui/tap-or-hover-hint';
import { hexBytes } from '@/lib/hex-bytes';
import {
  asciiPreview,
  bytePieces,
  fieldShape,
  hexWords,
  moleculeTableHeader,
} from '@/lib/byte-map';
import { usePinnedReadout } from '@/hooks/usePinnedReadout';
import { useDetailTier } from '@/components/ui/detail-layout';
import { cn } from '@/lib/utils';

export interface ProtocolEvidenceField {
  name: string;
  start: number;
  end: number;
  valueHex: string;
  /** Lane E (C5), additive: one sentence of what the field means in its protocol. */
  meaning?: string;
}

export interface ProtocolEvidenceProps {
  title: string;
  dataHex: string;
  fields: readonly ProtocolEvidenceField[];
  decoded?: unknown;
  /** Shown under the field detail (the cell it came from, who holds it). */
  children?: ReactNode;
}

/** Inks of the fields, in order (the Main board's cluster cell: name, description, …). */
const FIELD_INKS = ['#7aa7d6', '#5f9fb2', '#a98cf5', '#b5c77a'] as const;
const EMPTY_INK = '#343c50';
/** A field longer than this shows its first FOLDED_BYTES and folds the rest (C5). */
const WHOLE_LIMIT = 64;
const FOLDED_BYTES = 32;
const ROW = 16;

const fmt = (n: number) => n.toLocaleString('en-US');
const plural = (n: number, one: string, many: string) => `${fmt(n)} ${n === 1 ? one : many}`;
const sentence = (s: string) => `${s[0].toUpperCase()}${s.slice(1)}.`;

/** A run no field describes: said plainly, with what its bytes show when they show more than a count. */
function gapWords(own: readonly string[]): string {
  const shape = fieldShape(own);
  const plain = `No decoded field covers ${own.length === 1 ? 'this byte' : 'these bytes'}`;
  return /^[\d,]+ bytes?$/.test(shape) ? `${plain}.` : `${plain}: ${shape}.`;
}

interface Reading {
  key: string;
  name: string;
  start: number;
  end: number;
  words: string;
  ink: string;
  /** Leading bytes drawn dim: a length prefix, framing. */
  dim: number;
}

/**
 * A protocol cell's data, byte by byte (consistency C5, Main board "The cluster cell, byte by
 * byte"): every displayed field is checked against its actual byte slice first; then the bytes are
 * drawn to scale with the fields inside, an offsets ruler beneath, and one detail box that reads
 * the hovered or tapped field — its range, what it means, its bytes as 4-byte words with their
 * ASCII. A field longer than 64 bytes shows 32 and folds the rest; the whole data as hex, with
 * copy, is folded at the end.
 */
export function ProtocolEvidence(props: ProtocolEvidenceProps) {
  // Other bytes (a version switch) start the reading over: no hover, pin or copy result carries.
  return <ProtocolEvidenceView key={props.dataHex} {...props} />;
}

function ProtocolEvidenceView({
  title,
  dataHex,
  fields,
  decoded,
  children,
}: ProtocolEvidenceProps) {
  const bytes = hexBytes(dataHex, title);
  fields.forEach((field) => {
    if (
      !field.name.trim() ||
      !Number.isSafeInteger(field.start) ||
      !Number.isSafeInteger(field.end) ||
      field.start < 0 ||
      field.end < field.start ||
      field.end > bytes.length
    )
      throw new Error(
        `${title}: invalid field ${field.name} range [${field.start}, ${field.end}) for ${bytes.length} bytes`
      );
    const actual = bytes.slice(field.start, field.end).join('');
    const declared = hexBytes(field.valueHex, `${title}: field ${field.name}`).join('');
    if (declared !== actual)
      throw new Error(
        `${title}: field ${field.name} value does not match bytes [${field.start}, ${field.end})`
      );
  });
  const pieces = bytePieces(
    bytes.length,
    fields.map((f, i) => ({ id: `field-${i}`, start: f.start, end: f.end })),
    title
  );
  const gaps = pieces.filter((p) => p.ids.length === 0 && p.end > p.start);
  const undescribed = gaps.reduce((n, p) => n + p.end - p.start, 0);
  // ID-C12 / TX-16: inside tier 2 the heading is quiet, as DetailSection's are.
  const tier = useDetailTier();
  const readout = usePinnedReadout<string>();
  const [copyResult, setCopyResult] = useState<{ dataHex: string; error?: string } | null>(null);
  const hexFields: HexField[] = fields.map((field, index) => ({
    id: `field-${index}`,
    label: field.name,
    offset: field.start,
    length: field.end - field.start,
    description: `${field.name} · source bytes [${field.start}, ${field.end}) · ${field.valueHex}`,
    color: field.start === field.end ? EMPTY_INK : FIELD_INKS[index % FIELD_INKS.length],
  }));
  const fieldStarts = [...new Set(fields.map((f) => f.start))].sort((a, b) => a - b);

  const read = (key: string): Reading => {
    if (key.startsWith('gap:')) {
      const gap = gaps.find((p) => gapId(p.start, p.end) === key);
      if (!gap) throw new Error(`${title}: no undescribed byte run ${key}`);
      const own = bytes.slice(gap.start, gap.end);
      const header = gap.start === 0 ? moleculeTableHeader(bytes, gap.end, fieldStarts) : null;
      const n = gap.end - gap.start;
      return {
        key,
        name: header
          ? 'molecule table header'
          : plural(n, 'byte no field describes', 'bytes no field describes'),
        start: gap.start,
        end: gap.end,
        words: header
          ? `No field describes these bytes: a molecule table header, the size of all ${fmt(header.total)} bytes, then where its ${fmt(header.offsets.length)} fields start: ${header.offsets.map(fmt).join(', ')}.`
          : gapWords(own),
        ink: '#7c849c',
        dim: 0,
      };
    }
    const index = Number(key.slice('field-'.length));
    const field = fields[index];
    if (!field) throw new Error(`${title}: no field ${key}`);
    const own = bytes.slice(field.start, field.end);
    const shape = fieldShape(own);
    return {
      key,
      name: field.name,
      start: field.start,
      end: field.end,
      words: field.meaning ? `${field.meaning} ${sentence(shape)}` : sentence(shape),
      ink: hexFields[index].color!,
      dim: shape.startsWith('a 4-byte length') ? 4 : 0,
    };
  };
  const shownKey =
    readout.active ??
    (fields.length ? 'field-0' : gaps[0] ? gapId(gaps[0].start, gaps[0].end) : null);
  const reading = shownKey === null ? null : read(shownKey);

  const decodedJson =
    decoded === undefined
      ? undefined
      : JSON.stringify(
          decoded,
          (_key, value) => {
            // The API tags large CBOR integers as decimal strings. JSON numbers
            // can also be original CBOR floats, including large finite doubles;
            // preserve them rather than guessing their original numeric type.
            if (typeof value === 'number' && !Number.isFinite(value))
              throw new Error(`${title}: decoded numeric value is not finite`);
            return value;
          },
          2
        );
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(dataHex);
      setCopyResult({ dataHex });
    } catch (error) {
      setCopyResult({ dataHex, error: error instanceof Error ? error.message : String(error) });
    }
  };
  const copied = copyResult?.dataHex === dataHex ? copyResult : null;
  return (
    <section aria-label={title} className="min-w-0 space-y-3.5">
      <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1.5">
        <h2
          className={cn(
            'font-medium',
            tier === 2 ? 'text-text text-body' : 'text-text-bright text-section'
          )}
        >
          {title}
        </h2>
        <span className="text-text-dim text-meta">
          {plural(bytes.length, 'byte', 'bytes')} ·{' '}
          {plural(fields.length, 'described field', 'described fields')} ·{' '}
          {plural(undescribed, 'byte', 'bytes')} no field describes
        </span>
      </div>
      {bytes.length === 0 ? (
        <p className="text-text-dim text-small">Empty cell data (0 bytes).</p>
      ) : (
        <HexMap
          layout="scale"
          hex={dataHex}
          fields={hexFields}
          subject={title}
          activeField={readout.active}
          onFieldHover={readout.hover}
          onFieldSelect={readout.toggle}
        />
      )}
      {reading && (
        <FieldDetail
          title={title}
          reading={reading}
          bytes={bytes}
          pinned={readout.showingPin}
          hint={bytes.length > 0}
        />
      )}
      {children}
      {decodedJson !== undefined && (
        <details className="min-w-0">
          <summary className="text-text-dim text-small cursor-pointer">Decoded fields</summary>
          <p className="text-text-dim text-meta mt-3">
            Protocol interpretation; raw cell bytes remain the evidence.
          </p>
          <TintedJson text={decodedJson} className="mt-2" />
        </details>
      )}
      <details className="min-w-0">
        <summary className="text-text-dim text-small cursor-pointer">
          All {plural(bytes.length, 'byte', 'bytes')} as hex
        </summary>
        <div className="mt-3 flex flex-wrap items-baseline gap-3">
          <button
            type="button"
            onClick={copy}
            className="text-interactive hover:text-interactive-hover text-meta"
          >
            Copy raw bytes
          </button>
          {copied && (
            <span
              role={copied.error ? 'alert' : 'status'}
              className="text-text-dim text-small break-words"
            >
              {copied.error ? `Could not copy raw bytes: ${copied.error}` : 'Raw bytes copied.'}
            </span>
          )}
        </div>
        <pre className="text-text text-small mt-2 max-w-full whitespace-pre-wrap break-all">
          {dataHex}
        </pre>
      </details>
    </section>
  );
}

/**
 * The detail box (Main board): the field's name and `[a..b) · N B`, one sentence, then its bytes —
 * rows of 16 as 4-byte words with their ASCII; past 64 bytes only the first 32, the rest folded.
 */
function FieldDetail({
  title,
  reading,
  bytes,
  pinned,
  hint,
}: {
  title: string;
  reading: Reading;
  bytes: readonly string[];
  pinned: boolean;
  hint: boolean;
}) {
  const n = reading.end - reading.start;
  const shown = n > WHOLE_LIMIT ? FOLDED_BYTES : n;
  const digits = bytes.length > 0xffff ? 5 : 4;
  const rows: { at: number; bytes: string[] }[] = [];
  for (let at = 0; at < shown; at += ROW)
    rows.push({
      at,
      bytes: bytes.slice(reading.start + at, reading.start + Math.min(at + ROW, shown)),
    });
  return (
    <aside
      aria-label={`${title} field detail`}
      aria-live="polite"
      data-detail={reading.key}
      className="min-h-[96px] min-w-0 rounded-md border border-[#1a1f30] px-3.5 py-3"
    >
      <div className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-0.5">
        <span className="text-text-bright text-body min-w-0 [overflow-wrap:anywhere]">
          {pinned && <span className="text-text-dim">selected · </span>}
          <span>{reading.name}</span>
        </span>
        <span className="text-text-dim text-meta whitespace-nowrap tabular-nums">
          [{fmt(reading.start)}..{fmt(reading.end)}) · {fmt(n)} B
        </span>
      </div>
      <p className="text-text-dim text-small mt-1 [overflow-wrap:anywhere]">{reading.words}</p>
      {rows.map((row) => {
        const words = hexWords(row.bytes);
        return (
          <div
            key={row.at}
            data-hex-row=""
            className="text-small mt-1.5 grid grid-cols-[52px_auto_minmax(0,1fr)] items-baseline gap-x-3 tabular-nums"
          >
            <span className="text-[#4b5266]">
              0x{(reading.start + row.at).toString(16).padStart(digits, '0')}
            </span>
            <span className="flex flex-wrap gap-x-2">
              {words.map((word, i) => {
                const dim = row.at === 0 && i * 4 < reading.dim;
                return (
                  <span key={i} data-dim={dim} style={{ color: dim ? '#4b5266' : reading.ink }}>
                    {word}
                  </span>
                );
              })}
            </span>
            <span className="truncate text-[#a0a8be]">{asciiPreview(row.bytes)}</span>
          </div>
        );
      })}
      {n > WHOLE_LIMIT && (
        <>
          <p className="text-text-dim text-meta mt-1.5">… {fmt(n - shown)} more bytes</p>
          <details className="mt-1.5 min-w-0">
            <summary className="text-text-dim text-meta cursor-pointer">
              All {fmt(n)} bytes of {reading.name}
            </summary>
            <pre className="text-text text-small mt-2 max-w-full whitespace-pre-wrap break-all">
              0x{bytes.slice(reading.start, reading.end).join('')}
            </pre>
          </details>
        </>
      )}
      {hint && (
        <p className="text-text-dim text-meta mt-2 text-right">
          <TapOrHoverHint what="a field" />
        </p>
      )}
    </aside>
  );
}
