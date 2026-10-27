'use client';

import { useState, type ReactNode } from 'react';
import { HexMap, type HexField } from '@/components/ui/hex-map';
import { hexBytes } from '@/lib/hex-bytes';
import { CELL_COLORS } from '@/lib/design-tokens';

export interface ProtocolEvidenceProps {
  title: string;
  dataHex: string;
  fields: readonly { name: string; start: number; end: number; valueHex: string }[];
  decoded?: unknown;
  children?: ReactNode;
}

const PAGE_BYTES = 256;

/** Every displayed field is checked against its actual byte slice first. */
export function ProtocolEvidence({
  title,
  dataHex,
  fields,
  decoded,
  children,
}: ProtocolEvidenceProps) {
  const bytes = hexBytes(dataHex, title);
  const coverage = new Uint8Array(bytes.length);
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
    coverage.fill(1, field.start, field.end);
  });
  const unexplained = coverage.reduce((total, covered) => total + (covered === 0 ? 1 : 0), 0);
  const [selection, setSelection] = useState({ dataHex, page: 0 });
  const [active, setActive] = useState<string | null>(null);
  const [copyResult, setCopyResult] = useState<{ dataHex: string; error?: string } | null>(null);
  const page = selection.dataHex === dataHex ? selection.page : 0;
  const start = page * PAGE_BYTES;
  const end = Math.min(start + PAGE_BYTES, bytes.length);
  const visibleFields: HexField[] = fields.flatMap((field, index) => {
    const left = Math.max(start, field.start);
    const right = Math.min(end, field.end);
    if (left >= right) return [];
    return [
      {
        id: `field-${index}`,
        label: field.name,
        offset: left - start,
        length: right - left,
        description: `${field.name} · source bytes [${field.start}, ${field.end}) · ${field.valueHex}`,
        color: CELL_COLORS.data,
      },
    ];
  });
  const selectedIndex = active === null ? null : Number(active.slice('field-'.length));
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
  const changePage = (next: number) => {
    setSelection({ dataHex, page: next });
    setActive(null);
  };
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
    <section aria-label={title} className="min-w-0 space-y-4">
      <div className="flex flex-wrap items-baseline justify-between gap-3">
        <h2 className="text-text-bright text-[15px] font-medium">{title}</h2>
        <button
          type="button"
          onClick={copy}
          className="text-interactive hover:text-interactive-hover text-[11px]"
        >
          Copy raw bytes
        </button>
      </div>
      {copied && (
        <p role={copied.error ? 'alert' : 'status'} className="text-text-dim break-words text-xs">
          {copied.error ? `Could not copy raw bytes: ${copied.error}` : 'Raw bytes copied.'}
        </p>
      )}
      <p className="text-text-dim text-[11px]">
        {bytes.length.toLocaleString('en-US')} bytes · {fields.length.toLocaleString('en-US')}{' '}
        described fields · {unexplained.toLocaleString('en-US')} unexplained bytes
      </p>
      {children}
      {bytes.length === 0 ? (
        <p className="text-text-dim text-xs">Empty cell data (0 bytes).</p>
      ) : (
        <div className="min-w-0 [&_button]:max-w-full [&_button]:break-all [&_p]:break-all">
          {bytes.length > PAGE_BYTES && (
            <nav
              aria-label={`${title} byte pages`}
              className="mb-3 flex flex-wrap items-center gap-3 text-[11px]"
            >
              <button
                type="button"
                disabled={page === 0}
                onClick={() => changePage(page - 1)}
                className="disabled:opacity-40"
              >
                Previous bytes
              </button>
              <span>
                Bytes {start}–{end - 1} of {bytes.length}
              </span>
              <button
                type="button"
                disabled={end === bytes.length}
                onClick={() => changePage(page + 1)}
                className="disabled:opacity-40"
              >
                Next bytes
              </button>
            </nav>
          )}
          <HexMap
            key={`${page}:${bytes.length}`}
            hex={bytes.slice(start, end).join('')}
            fields={visibleFields}
            subject={title}
            offsetBase={start}
            onFieldHover={setActive}
          />
        </div>
      )}
      {fields.length > 0 && (
        <dl aria-label={`${title} field values`} className="grid min-w-0 gap-3 text-xs">
          {fields.map((field, index) => (
            <div
              key={`${index}:${field.name}`}
              data-field-index={index}
              data-active={selectedIndex === index}
              className={
                selectedIndex === index ? 'bg-base-elevated min-w-0 rounded p-2' : 'min-w-0 p-2'
              }
            >
              <dt className="text-text-dim break-words text-[11px]">
                {field.name} · [{field.start}, {field.end})
                {field.start === field.end ? ' · empty field' : ''}
              </dt>
              <dd className="text-text break-all font-mono">{field.valueHex}</dd>
            </div>
          ))}
        </dl>
      )}
      {decodedJson !== undefined && (
        <details className="min-w-0">
          <summary className="text-text-dim cursor-pointer text-xs">Decoded fields</summary>
          <p className="text-text-dim mt-3 text-[11px]">
            Protocol interpretation; raw cell bytes remain the evidence.
          </p>
          <pre className="text-text mt-2 max-w-full whitespace-pre-wrap break-all text-xs">
            {decodedJson}
          </pre>
        </details>
      )}
      <details className="min-w-0">
        <summary className="text-text-dim cursor-pointer text-xs">Complete raw hex</summary>
        <pre className="text-text mt-3 max-w-full whitespace-pre-wrap break-all text-xs">
          {dataHex}
        </pre>
      </details>
    </section>
  );
}
