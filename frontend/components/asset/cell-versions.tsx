'use client';
import { unitText } from '@/components/ui/units';
import { formatCount } from '@/lib/format-count';
import { FoldToggle } from '@/components/ui/fold-toggle';
import { transactionAtCellHref } from '@/lib/tx-cell-link';
import { useId, useState, type ReactNode } from 'react';
import type { ItemCellVersions, ItemCellVersion } from '@/lib/api';
import { AppLink } from '@/components/ui/app-link';
import { CellBar } from '@/components/ui/cell-bar';
import { Hash } from '@/components/ui/hash';
import { DetailSection } from '@/components/ui/detail-layout';
import { CkbAmount } from '@/components/ui/ckb-amount';
import { exactUnsignedInteger } from '@/lib/exact-integer';
import { cellOwnerLockPart } from '@/components/ui/lock-kind';
import { cn } from '@/lib/utils';
import { formatBlockNumber } from '@/lib/format-block';
import { versionFree } from '@/lib/held-point';
export interface CellVersionsProps {
  history: ItemCellVersions;
  subject: string;
  // ── F2 objects (round 2, OB-9 / OB-M7): the Object / Mnft board's "CKB held in its cell" ──
  /**
   * Section title. By default the section is the versions themselves, "Cell versions" run on
   * their count (rubbing × Song print, phase 5, D13); a caller that names it otherwise (the
   * boards' "CKB held") gives the whole heading.
   */
  title?: string;
  actions?: ReactNode;
  intro?: ReactNode;
  /**
   * Label the first cell "At minting" and the live one "Now", draw each bar to scale against the
   * largest cell with its free part, as the Object and Mnft boards do.
   */
  life?: boolean;
  /** The first cell's label in `life` mode ("At minting"; "Issued" for an m-NFT token). */
  firstLabel?: string;
  /**
   * F2 identities (round 2): what the cell's lock address is to this item. A `.cell` name is held
   * under a lock every name shares; its owner is in the data, so the line says "Lock" there.
   */
  lockAddressLabel?: string;
}
export function CellVersions({
  history,
  subject,
  title,
  actions,
  intro,
  life = false,
  firstLabel = 'At minting',
  lockAddressLabel = 'Owner',
}: CellVersionsProps) {
  const [active, setActive] = useState<string | null>(null);
  const [opened, setOpened] = useState<ReadonlySet<string>>(new Set());
  const foldId = useId();
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
        ? firstLabel
        : v.isLive
          ? 'Now'
          : 'Moved'
      : `${i === 0 ? 'Minted cell' : `Version ${i + 1}`}${v.isLive ? ' · current' : ''}`;
  const row = (v: ItemCellVersion, i: number) => {
    const key = `${v.txHash}:${v.outputIndex}`;
    // the version's free part, the route's (lane exactness, part B5: it was capacity − occupied, here)
    const free = versionFree(v, `cell version ${key}`);
    return (
      <article
        key={key}
        tabIndex={0}
        onMouseEnter={() => setActive(key)}
        onMouseLeave={() => setActive(null)}
        onFocus={() => setActive(key)}
        onBlur={() => setActive(null)}
        className={cn(
          'border-row-rule text-small grid min-w-0 gap-3 border-b py-3 sm:grid-cols-[130px_minmax(0,1fr)_160px]',
          active && active !== key && 'opacity-35'
        )}
      >
        <div className="min-w-0">
          <AppLink look="key" href={`/cell/${v.txHash}-${v.outputIndex}`}>
            {label(v, i)}
          </AppLink>
          <AppLink look="inline" href={`/blocks/${v.blockNumber}`} className="mt-1 block">
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
              <div className="text-text-dim text-meta mt-1.5 flex flex-wrap gap-x-4">
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
          {/* Lane P2's third round (item 2): the owner where it changes from the version before
              (the first version names it); a version under the same owner says nothing of it */}
          {(i === 0 || history.data[i - 1].ownerAddress !== v.ownerAddress) && (
            <div className="text-text-dim mt-2 flex flex-wrap gap-x-3 gap-y-1">
              <span>
                {lockAddressLabel}{' '}
                <AppLink look="inline" href={`/address/${v.ownerAddress}`}>
                  <Hash hash={v.ownerAddress} kind="address" size="row" className="text-meta" />
                </AppLink>
              </span>
              {/* the owner's lock by the one decision (lane B4; lane p-assets: it wrote the
                  API's name, "Lock" where it had none, and the code hash): nothing for the
                  default lock */}
              {cellOwnerLockPart(v)}
            </div>
          )}
        </div>
        <div className="text-text-bright min-w-0 sm:text-right">
          <CkbAmount form="exact" shannons={BigInt(v.capacity)} signed={false} tone="plain" wrap />
          <div className="text-text-dim text-meta mt-1">
            <CkbAmount form="exact" shannons={free} signed={false} tone="plain" wrap /> free
          </div>
          {v.consumedByTx && (
            <AppLink
              look="onward"
              // the spending transaction opened at this version's cell, so its fold
              // never hides it
              href={transactionAtCellHref(v.consumedByTx, {
                txHash: v.txHash,
                index: v.outputIndex,
              })}
              className="text-meta mt-1 block"
            >
              Consumed in transaction
            </AppLink>
          )}
        </div>
      </article>
    );
  };
  // Lane P2's third round (item 3): consecutive versions that differ in nothing a reader can see
  // (capacity, bytes, owner) fold: the run's first stands, and the current version, and those
  // between them fold under the one fold control, two or more of them; a version that changes
  // anything starts a run of its own and is never folded
  const alike = (a: ItemCellVersion, b: ItemCellVersion) =>
    a.capacity === b.capacity &&
    a.anatomy.lockBytes === b.anatomy.lockBytes &&
    a.anatomy.typeBytes === b.anatomy.typeBytes &&
    a.anatomy.dataBytes === b.anatomy.dataBytes &&
    a.ownerAddress === b.ownerAddress;
  type Segment =
    | { kind: 'row'; v: ItemCellVersion; i: number }
    | { kind: 'fold'; id: string; hidden: { v: ItemCellVersion; i: number }[] };
  const segments: Segment[] = [];
  for (let i = 0; i < history.data.length; ) {
    let j = i;
    while (j + 1 < history.data.length && alike(history.data[j + 1], history.data[i])) j++;
    const keepLast = j > i && (history.data[j].isLive || j === history.data.length - 1);
    const hidden = history.data
      .slice(i + 1, keepLast ? j : j + 1)
      .map((v, k) => ({ v, i: i + 1 + k }));
    segments.push({ kind: 'row', v: history.data[i], i });
    if (hidden.length >= 2) segments.push({ kind: 'fold', id: `${foldId}-${i}`, hidden });
    else for (const h of hidden) segments.push({ kind: 'row', ...h });
    if (keepLast) segments.push({ kind: 'row', v: history.data[j], i: j });
    i = j + 1;
  }
  return (
    <DetailSection
      title={title ?? 'Cell versions'}
      args={title === undefined ? history.total.toLocaleString('en-US') : undefined}
      actions={
        actions ?? (
          <span className="text-text-dim text-meta">
            Every canonical cell version · oldest first
          </span>
        )
      }
    >
      {intro}
      <div aria-label={`${subject} cell versions`} className="space-y-3">
        {segments.map((segment) =>
          segment.kind === 'row' ? (
            row(segment.v, segment.i)
          ) : (
            <VersionsFold
              key={segment.id}
              id={segment.id}
              hidden={segment.hidden}
              open={opened.has(segment.id)}
              onToggle={(open) =>
                setOpened((was) => {
                  const next = new Set(was);
                  if (open) next.add(segment.id);
                  else next.delete(segment.id);
                  return next;
                })
              }
            >
              {segment.hidden.map(({ v, i }) => row(v, i))}
            </VersionsFold>
          )
        )}
        {history.data.length === 0 && (
          <p className="text-text-dim text-small">No canonical cell versions.</p>
        )}
      </div>
    </DetailSection>
  );
}

/**
 * Versions alike folded (lane P2's third round, item 3): one row saying how many and what they
 * share, "2 more versions alike: 500 CKB held, the same bytes and owner", and the one fold control,
 * "all 2 versions ↓" / "fewer ↑", which opens them in place, the button kept and its focus with it.
 */
function VersionsFold({
  id,
  hidden,
  open,
  onToggle,
  children,
}: {
  id: string;
  hidden: readonly { v: ItemCellVersion; i: number }[];
  open: boolean;
  onToggle: (open: boolean) => void;
  children: ReactNode;
}) {
  const shared = hidden[0].v;
  return (
    <div data-versions-fold="" className="border-row-rule text-small min-w-0 border-b py-3">
      <p className="text-text max-w-measure">
        {unitText(formatCount(hidden.length, 'more version', 'more versions'))} alike:{' '}
        <CkbAmount form="summary" shannons={BigInt(shared.capacity)} signed={false} tone="plain" />{' '}
        held, the same bytes and owner
      </p>
      <div id={id} className="space-y-3">
        {open && children}
      </div>
      <FoldToggle
        open={open}
        count={hidden.length}
        singular="version"
        controls={id}
        onToggle={onToggle}
      />
    </div>
  );
}
