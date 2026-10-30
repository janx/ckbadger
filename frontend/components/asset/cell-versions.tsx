'use client';
import { useState, type ReactNode } from 'react';
import type { ItemCellVersions, ItemCellVersion } from '@/lib/api';
import { AppLink } from '@/components/ui/app-link';
import { CellBar } from '@/components/ui/cell-bar';
import { Hash } from '@/components/ui/hash';
import { DetailSection } from '@/components/ui/detail-layout';
import { ExactCkb } from '@/components/activity-event-row';
import { exactUnsignedInteger } from '@/lib/exact-integer';
import { getScriptDetailHref } from '@/lib/detail-routes';
import { cn } from '@/lib/utils';
import { formatBlockNumber } from '@/lib/format-block';
export interface CellVersionsProps {
  history: ItemCellVersions;
  subject: string;
  // ── F2 objects (round 2, OB-9 / OB-M7): the Object / Mnft board's "CKB held in its cell" ──
  /** Section title; "Cell versions" by default. */
  title?: string;
  actions?: ReactNode;
  intro?: ReactNode;
  /**
   * Label the first cell "At minting" and the live one "Now", draw each bar to scale against the
   * largest cell with its free part, as the Object and Mnft boards do.
   */
  life?: boolean;
}
export function CellVersions({
  history,
  subject,
  title = 'Cell versions',
  actions,
  intro,
  life = false,
}: CellVersionsProps) {
  const [active, setActive] = useState<string | null>(null);
  if (history.total !== history.data.length)
    throw new Error(
      `${subject}: incomplete cell-version history (${history.data.length}/${history.total})`
    );
  let previous: ItemCellVersion | undefined;
  for (const v of history.data) {
    const position = `${v.txHash}:${v.outputIndex}`;
    if (
      previous &&
      (v.blockNumber < previous.blockNumber ||
        (v.blockNumber === previous.blockNumber && v.txIndex < previous.txIndex))
    )
      throw new Error(`${subject}: cell versions out of order at ${position}`);
    previous = v;
    if (
      Object.values(v.anatomy).some((n) => !Number.isSafeInteger(n) || n < 0) ||
      v.anatomy.capacityBytes !== 8
    )
      throw new Error(`${position}: invalid cell anatomy`);
    const bytes = Object.values(v.anatomy).reduce((sum, n) => sum + BigInt(n), BigInt(0));
    const occupied = exactUnsignedInteger(v.occupiedCapacity, `${position} occupied`);
    if (bytes * BigInt(100000000) !== occupied)
      throw new Error(`${position}: anatomy does not equal occupied capacity`);
    if (occupied > exactUnsignedInteger(v.capacity, `${position} capacity`))
      throw new Error(`${position}: occupied exceeds capacity`);
  }
  const largest = history.data.reduce(
    (max, v) => (BigInt(v.capacity) > max ? BigInt(v.capacity) : max),
    BigInt(0)
  );
  const scaled = (capacity: string) =>
    largest === BigInt(0)
      ? '0%'
      : `${Number(((BigInt(capacity) * BigInt(200000)) / largest + BigInt(1)) / BigInt(2)) / 1000}%`;
  const label = (v: ItemCellVersion, i: number) =>
    life
      ? i === 0
        ? 'At minting'
        : v.isLive
          ? 'Now'
          : `Version ${i + 1}`
      : `${i === 0 ? 'Minted cell' : `Version ${i + 1}`}${v.isLive ? ' · current' : ''}`;
  return (
    <DetailSection
      title={title}
      actions={
        actions ?? (
          <span className="text-text-dim text-[11px]">
            Every canonical cell version · oldest first
          </span>
        )
      }
    >
      {intro}
      <div aria-label={`${subject} cell versions`} className="space-y-3">
        {history.data.map((v, i) => {
          const key = `${v.txHash}:${v.outputIndex}`;
          const free = BigInt(v.capacity) - BigInt(v.occupiedCapacity);
          return (
            <article
              key={key}
              tabIndex={0}
              onMouseEnter={() => setActive(key)}
              onMouseLeave={() => setActive(null)}
              onFocus={() => setActive(key)}
              onBlur={() => setActive(null)}
              className={cn(
                'border-base-border-subtle grid min-w-0 gap-3 border-b py-3 text-xs sm:grid-cols-[130px_minmax(0,1fr)_160px]',
                active && active !== key && 'opacity-35'
              )}
            >
              <div className="min-w-0">
                <AppLink className="text-text-bright" href={`/cell/${v.txHash}-${v.outputIndex}`}>
                  {label(v, i)}
                </AppLink>
                <AppLink href={`/blocks/${v.blockNumber}`} className="text-text-dim mt-1 block">
                  {formatBlockNumber(v.blockNumber)}
                </AppLink>
              </div>
              <div className="min-w-0">
                {life ? (
                  <div data-scaled style={{ width: scaled(v.capacity) }}>
                    <CellBar
                      subject={key}
                      capacity={v.capacity}
                      anatomy={{
                        lock: String(v.anatomy.lockBytes),
                        type: String(v.anatomy.typeBytes),
                        data: String(v.anatomy.dataBytes),
                      }}
                    />
                    <div className="text-text-dim mt-1.5 flex flex-wrap gap-x-4 text-[11px]">
                      <span>capacity 8</span>
                      <span>lock {v.anatomy.lockBytes}</span>
                      <span>type {v.anatomy.typeBytes}</span>
                      <span>data {v.anatomy.dataBytes}</span>
                    </div>
                  </div>
                ) : (
                  <CellBar
                    subject={key}
                    anatomy={{
                      lock: String(v.anatomy.lockBytes),
                      type: String(v.anatomy.typeBytes),
                      data: String(v.anatomy.dataBytes),
                    }}
                  />
                )}
                <div className="text-text-dim mt-2 flex flex-wrap gap-x-3 gap-y-1">
                  <span>
                    Owner{' '}
                    <AppLink href={`/address/${v.ownerAddress}`}>
                      <Hash
                        hash={v.ownerAddress}
                        copyable={false}
                        startChars={10}
                        endChars={6}
                        className="text-[11px]"
                      />
                    </AppLink>
                  </span>
                  <AppLink href={getScriptDetailHref({ ...v.lock, scriptKind: 'lock' })}>
                    {v.lockName ?? 'Lock'}{' '}
                    <Hash
                      hash={v.lock.codeHash}
                      startChars={6}
                      endChars={4}
                      className="text-[11px]"
                      copyable={false}
                    />
                  </AppLink>
                </div>
              </div>
              <div className="text-text-bright min-w-0 sm:text-right">
                <ExactCkb shannons={BigInt(v.capacity)} signed={false} tone="plain" wrap />
                <div className="text-text-dim mt-1 text-[11px]">
                  <ExactCkb shannons={free} signed={false} tone="plain" wrap /> free
                </div>
                {v.consumedByTx && (
                  <AppLink
                    href={`/tx/${v.consumedByTx}`}
                    className="text-text-dim mt-1 block text-[11px]"
                  >
                    Consumed in transaction →
                  </AppLink>
                )}
              </div>
            </article>
          );
        })}
        {history.data.length === 0 && (
          <p className="text-text-dim text-xs">No canonical cell versions.</p>
        )}
      </div>
    </DetailSection>
  );
}
