import { CELL_COLORS } from '@/lib/design-tokens';
import { exactUnsignedInteger } from '@/lib/exact-integer';

export interface CellAnatomy {
  lock: string;
  type: string;
  data: string;
}
export interface CellBarProps {
  subject: string;
  anatomy: CellAnatomy;
  className?: string;
}
/** Bytes, including the mandatory eight-byte capacity field. No inferred script sizes. */
export function CellBar({ subject, anatomy, className }: CellBarProps) {
  const parts = [
    { key: 'capacity' as const, value: BigInt(8) },
    ...(['lock', 'type', 'data'] as const).map((key) => ({
      key,
      value: exactUnsignedInteger(anatomy[key], `${subject}: ${key} bytes`),
    })),
  ];
  const total = parts.reduce((s, p) => s + p.value, BigInt(0));
  return (
    <div className={className}>
      <div
        className="flex h-2 w-full"
        role="img"
        aria-label={`${subject}: ${total} occupied bytes`}
      >
        {parts.map((p) => (
          <span
            key={p.key}
            data-part={p.key}
            title={`${p.key}: ${p.value} bytes`}
            style={{
              width: `${(Number(p.value) / Number(total)) * 100}%`,
              backgroundColor: CELL_COLORS[p.key],
            }}
          />
        ))}
      </div>
      <div className="text-text-dim mt-2 flex flex-wrap gap-x-4 gap-y-1 text-[11px]">
        {parts.map((p) => (
          <span key={p.key} title={`${p.key}: ${p.value} bytes`}>
            <span style={{ color: CELL_COLORS[p.key] }}>{p.key}</span> {p.value.toString()} B
          </span>
        ))}
      </div>
    </div>
  );
}
