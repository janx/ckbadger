import { CELL_COLORS, FREE_CAPACITY_COLOR } from '@/lib/design-tokens';
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
  // group scripts (F2-5): the cell's capacity in shannons; the bar then draws the occupied
  // bytes at their share of it and the rest as free (Script board "Its cells").
  capacity?: string;
  /** Hide the per-part byte legend under the bar (table rows carry it in the label). */
  hideLegend?: boolean;
}
/** Bytes, including the mandatory eight-byte capacity field. No inferred script sizes. */
export function CellBar({ subject, anatomy, className, capacity, hideLegend }: CellBarProps) {
  const parts = [
    { key: 'capacity' as const, value: BigInt(8) },
    ...(['lock', 'type', 'data'] as const).map((key) => ({
      key,
      value: exactUnsignedInteger(anatomy[key], `${subject}: ${key} bytes`),
    })),
  ];
  const total = parts.reduce((s, p) => s + p.value, BigInt(0));
  if (capacity !== undefined) return freeBar(subject, parts, total, capacity, className);
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
      {!hideLegend && (
        <div className="text-text-dim mt-2 flex flex-wrap gap-x-4 gap-y-1 text-[11px]">
          {parts.map((p) => (
            <span key={p.key} title={`${p.key}: ${p.value} bytes`}>
              <span style={{ color: CELL_COLORS[p.key] }}>{p.key}</span> {p.value.toString()} B
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

// group scripts (F2-5): occupied bytes at their share of the capacity, the rest free.
function freeBar(
  subject: string,
  parts: { key: keyof typeof CELL_COLORS; value: bigint }[],
  total: bigint,
  capacity: string,
  className: string | undefined
) {
  const shannons = exactUnsignedInteger(capacity, `${subject}: capacity`);
  const occupied = total * BigInt(100000000);
  if (occupied > shannons)
    throw new Error(`${subject}: ${total} occupied bytes exceed capacity ${shannons} shannons`);
  const free = shannons - occupied;
  const share = Number((occupied * BigInt(1000000)) / shannons) / 10000;
  return (
    <span
      role="img"
      data-free-shannons={free.toString()}
      aria-label={`${subject}: ${total} occupied bytes of ${shannons} shannons; capacity ${parts[0].value}, lock ${parts[1].value}, type ${parts[2].value}, data ${parts[3].value} bytes`}
      className={`flex h-2 w-full overflow-hidden rounded-sm ${className ?? ''}`}
      style={{ backgroundColor: FREE_CAPACITY_COLOR }}
    >
      <span className="flex h-full" style={{ width: `${share}%`, minWidth: 2 }}>
        {parts.map((p) => (
          <span
            key={p.key}
            data-part={p.key}
            style={{
              width: `${(Number(p.value) / Number(total)) * 100}%`,
              backgroundColor: CELL_COLORS[p.key],
            }}
          />
        ))}
      </span>
      {free > BigInt(0) && <span data-part="free" className="flex-1" />}
    </span>
  );
}
