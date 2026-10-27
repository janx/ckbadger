'use client';
import { useState } from 'react';
import { hexBytes, validateByteRanges, type ByteRange } from '@/lib/hex-bytes';
import { cn } from '@/lib/utils';
export type HexField = ByteRange & { color?: string };
export function HexMap({
  hex,
  fields,
  subject,
  onFieldHover,
  offsetBase = 0,
}: {
  hex: string;
  fields: readonly HexField[];
  subject: string;
  onFieldHover?: (field: string | null) => void;
  /** Absolute position of the first byte when showing a page of a payload. */
  offsetBase?: number;
}) {
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
                'min-w-[2.5ch] rounded-sm px-0.5 py-1 text-xs tabular-nums',
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
              'text-left text-[11px]',
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
      <p aria-live="polite" className="text-text min-h-5 text-xs">
        {selected ? selected.description : `${bytes.length} bytes · inspect a field or byte`}
      </p>
    </div>
  );
}
