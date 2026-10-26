import { CLASS_COLORS, type AssetClass } from '@/lib/design-tokens';
import { cn } from '@/lib/utils';

export function ClassMark({
  kind,
  size = 14,
  className,
}: {
  kind: AssetClass;
  size?: number;
  className?: string;
}) {
  const shapes = {
    dao: (
      <>
        <path d="M3 12V6l5-3 5 3v6M2 13h12M5 7v4m3-4v4m3-4v4" />
      </>
    ),
    tokens: (
      <>
        <ellipse cx="8" cy="5" rx="5" ry="2" />
        <path d="M3 5v3c0 3 10 3 10 0V5M3 8v3c0 3 10 3 10 0V8" />
      </>
    ),
    objects: (
      <>
        <path d="m8 1 6 4v6l-6 4-6-4V5l6-4Z" />
        <path d="m2 5 6 4 6-4M8 9v6M8 1v8" />
      </>
    ),
    identities: (
      <>
        <circle cx="8" cy="5" r="3" />
        <path d="M2 15v-2a6 6 0 0 1 12 0v2" />
      </>
    ),
    ckb: <rect x="3" y="3" width="10" height="10" rx="1" />,
    other: <path d="m5 3-4 5 4 5m6-10 4 5-4 5" />,
  };
  return (
    <svg
      role="img"
      aria-label={`${kind} class`}
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      stroke={CLASS_COLORS[kind]}
      strokeWidth="1.2"
      className={cn('inline-block shrink-0 align-middle', className)}
    >
      {shapes[kind]}
    </svg>
  );
}
