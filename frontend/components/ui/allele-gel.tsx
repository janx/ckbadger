'use client';
import { useState } from 'react';
import { hexBytes, validateByteRanges, type ByteRange } from '@/lib/hex-bytes';
import { CLASS_COLORS } from '@/lib/design-tokens';
export interface DnaGene extends ByteRange {
  value: string;
  color?: string;
}
export function AlleleGel({
  dna,
  genes,
  subject,
  onGeneHover,
}: {
  dna: string;
  genes: readonly DnaGene[];
  subject: string;
  onGeneHover?: (id: string | null) => void;
}) {
  const bytes = hexBytes(dna, subject);
  validateByteRanges(genes, bytes.length, subject);
  const [active, setActive] = useState<string | null>(null);
  const digits = bytes.join('').split('');
  const select = (id: string | null) => {
    setActive(id);
    onGeneHover?.(id);
  };
  const h = 40 + digits.length * 3;
  return (
    <div className="grid min-w-0 grid-cols-1 items-start gap-6 sm:grid-cols-[240px_minmax(0,1fr)]">
      <svg
        role="img"
        aria-label={`${subject}: ${bytes.length}-byte DNA sequencing gel`}
        viewBox={`0 0 240 ${h}`}
        width="240"
        height={h}
        className="max-w-full"
      >
        {Array.from({ length: 16 }, (_, lane) => (
          <g key={lane}>
            <text x={30 + lane * 13} y="12" textAnchor="middle" fill="#7c849c" fontSize="10">
              {lane.toString(16)}
            </text>
            <line x1={30 + lane * 13} x2={30 + lane * 13} y1="24" y2={h - 5} stroke="#1a1f30" />
          </g>
        ))}
        {digits.map((digit, i) => {
          const owners = genes.filter(
            (g) => Math.floor(i / 2) >= g.offset && Math.floor(i / 2) < g.offset + g.length
          );
          return (
            <rect
              key={i}
              data-nibble={i}
              x={25 + parseInt(digit, 16) * 13}
              y={28 + i * 3}
              width="10"
              height="1.8"
              fill={owners[0]?.color ?? CLASS_COLORS.objects}
              opacity={active && !owners.some((g) => g.id === active) ? 0.15 : 1}
            />
          );
        })}
      </svg>
      <dl className="divide-base-elevated min-w-0 divide-y text-xs">
        {genes.map((g) => (
          <div
            key={g.id}
            tabIndex={0}
            onMouseEnter={() => select(g.id)}
            onMouseLeave={() => select(null)}
            onFocus={() => select(g.id)}
            onBlur={() => select(null)}
            data-gene={g.id}
            className="py-3"
          >
            <dt style={{ color: g.color ?? CLASS_COLORS.objects }}>
              {g.label}{' '}
              <span className="text-text-dim">
                bytes {g.offset}–{g.offset + g.length - 1}
              </span>
            </dt>
            <dd className="text-text-bright mt-1 break-words">
              {g.value}
              <span className="text-text-dim mt-1 block">{g.description}</span>
            </dd>
          </div>
        ))}
      </dl>
    </div>
  );
}
