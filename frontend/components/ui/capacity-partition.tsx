'use client';
import { useState } from 'react';
import { ExactCkb } from '@/components/activity-event-row';
import { exactUnsignedInteger, formatScaledInteger } from '@/lib/exact-integer';
import { DATA_COLOR, CLASS_COLORS } from '@/lib/design-tokens';

export interface CapacityPartitionProps {
  subject: string;
  /** All values are exact shannons supplied by the owning API calculation. */
  total: string;
  occupied: string;
  free: string;
  daoFree?: string;
  className?: string;
}
export function CapacityPartition({
  subject,
  total,
  occupied,
  free,
  daoFree,
  className,
}: CapacityPartitionProps) {
  const [active, setActive] = useState<string | null>(null);
  const sum = exactUnsignedInteger(total, `${subject}: total`);
  const parts = [
    {
      label: 'occupied',
      value: exactUnsignedInteger(occupied, `${subject}: occupied`),
      color: DATA_COLOR,
    },
    {
      label: 'free',
      value: exactUnsignedInteger(free, `${subject}: free`),
      color: CLASS_COLORS.ckb,
    },
    ...(daoFree === undefined
      ? []
      : [
          {
            label: 'DAO free',
            value: exactUnsignedInteger(daoFree, `${subject}: DAO free`),
            color: CLASS_COLORS.dao,
          },
        ]),
  ];
  if (parts.reduce((s, p) => s + p.value, BigInt(0)) !== sum)
    throw new Error(`${subject}: capacity partition does not equal total ${total}`);
  return (
    <div className={className} aria-label={`${subject} capacity partition`}>
      <div className="mb-3 flex flex-wrap justify-between gap-2 text-xs">
        <span className="text-text-dim">CKB held</span>
        <ExactCkb shannons={sum} signed={false} tone="plain" />
      </div>
      {sum > BigInt(0) && (
        <div className="flex h-2 w-full" aria-hidden="true">
          {parts.map((p) => (
            <div
              key={p.label}
              data-part={p.label}
              style={{
                width: `${(Number(p.value) / Number(sum)) * 100}%`,
                backgroundColor: p.color,
                opacity: active && active !== p.label ? 0.25 : 1,
              }}
            />
          ))}
        </div>
      )}
      <dl className="mt-3 flex flex-wrap gap-x-6 gap-y-3 text-xs">
        {parts.map((p) => (
          <div
            key={p.label}
            tabIndex={0}
            onMouseEnter={() => setActive(p.label)}
            onMouseLeave={() => setActive(null)}
            onFocus={() => setActive(p.label)}
            onBlur={() => setActive(null)}
          >
            <dt className="text-text-dim">{p.label}</dt>
            <dd>
              <ExactCkb shannons={p.value} signed={false} tone="plain" />
              {sum > BigInt(0) && (
                <span className="text-text-dim ml-2">
                  {formatScaledInteger(p.value * BigInt(100), sum, 2)}%
                </span>
              )}
            </dd>
          </div>
        ))}
      </dl>
    </div>
  );
}
