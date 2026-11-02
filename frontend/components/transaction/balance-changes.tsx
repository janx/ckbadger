'use client';
import type { ReactNode } from 'react';
import { ActivityReadError, ExactCkb, ItemDeltaFact } from '@/components/activity-event-row';
import { formatOccupiedDelta } from '@/components/activity/activity-row';
import { ParticipantRefView } from '@/components/participant-ref';
import { DetailSection } from '@/components/ui/detail-layout';
import { useElementWidth } from '@/hooks/useElementWidth';
import type { GlobalActivity, ParticipantInfo } from '@/lib/api';
import { balanceNet } from '@/lib/tx-detail-model';
import { cn, truncateHash } from '@/lib/utils';

function readable(txHash: string, render: () => ReactNode): ReactNode {
  try {
    return render();
  } catch (error) {
    return <ActivityReadError txHash={txHash} error={error} />;
  }
}

/** Each address's lock code hash, read off the transaction's own cells (inputs and outputs). */
export function lockCodeHashesByAddress(
  cells: readonly {
    address?: string;
    lock?: { codeHash: string; hashType: string; args: string };
  }[]
): ReadonlyMap<string, string> {
  const map = new Map<string, string>();
  for (const c of cells) {
    // A pending input has no resolved cell yet: no address, no lock to read.
    if (c.address === undefined || c.lock === undefined) continue;
    const seen = map.get(c.address);
    if (seen !== undefined && seen !== c.lock.codeHash)
      throw new Error(
        `address ${c.address} carries two lock code hashes: ${seen} and ${c.lock.codeHash}`
      );
    map.set(c.address, c.lock.codeHash);
  }
  return map;
}

/** The four columns' minimum: below it the table would have to scroll, so the cards are drawn. */
const TABLE_MIN_W = 700;
/** From here a card holds the party and its CKB change on one line (the 640 board). */
const TWO_LINE_MIN_W = 480;

type BalanceForm = 'table' | 'two-line' | 'three-line';

/**
 * The form the measured width holds. Until the box is measured (before the first layout) the
 * table is assumed, as every measured component here does; the browser measures before paint.
 */
function balanceForm(width: number | null): BalanceForm {
  if (width === null || width >= TABLE_MIN_W) return 'table';
  return width >= TWO_LINE_MIN_W ? 'two-line' : 'three-line';
}

/**
 * The participant's lock: its registry name, else its code hash (the transaction's cells carry
 * the script). A party with no cell here (a protocol-named party) or no known script has none.
 */
function LockLabel({
  participant,
  lockCodeHashes,
  className,
}: {
  participant: ParticipantInfo;
  lockCodeHashes: ReadonlyMap<string, string>;
  className?: string;
}) {
  const codeHash =
    participant.address === null ? undefined : lockCodeHashes.get(participant.address);
  const name = participant.lockKind ?? (codeHash ? `lock ${truncateHash(codeHash, 6, 4)}` : null);
  if (name === null) return null;
  return (
    <span
      data-role="lock"
      title={codeHash}
      className={cn('text-text-dim text-meta whitespace-nowrap', className)}
    >
      {name}
    </span>
  );
}

function Items({ participant }: { participant: ParticipantInfo }) {
  return (
    <>
      {participant.itemDeltas.map((item, j) => (
        <ItemDeltaFact key={j} item={item} />
      ))}
    </>
  );
}

const BALANCE_GRID = 'grid grid-cols-[190px_180px_100px_minmax(0,1fr)] gap-4';

function netNote(note: ReturnType<typeof balanceNet>['note']): string {
  return note === 'fee'
    ? 'the fee, paid to the miner'
    : note === 'compensation'
      ? 'Nervos DAO compensation, less the fee'
      : 'issued in the genesis block';
}

function partyKey(p: ParticipantInfo, i: number): string {
  return `${p.lockHash ?? p.lockHashPrefix ?? p.address}:${i}`;
}

/**
 * Balance changes (TX-12, TX-19): every participant, then the Net row that explains the sum.
 * The measured width decides the form (docs/RESPONSIVE_LAYOUT.md rule 4): the four-column table
 * where its 700 px fit, otherwise one card per participant — party and lock, CKB and occupied,
 * the items moved — with the Net row as the last card. Never a sideways scroller.
 */
export function BalanceChanges({
  activity,
  fee,
  compensation,
  genesis,
  lockCodeHashes,
}: {
  activity: GlobalActivity;
  fee: string;
  compensation: bigint | null;
  genesis: boolean;
  /** Address → lock code hash, from the transaction's cells (`lockCodeHashesByAddress`). */
  lockCodeHashes: ReadonlyMap<string, string>;
}) {
  const [ref, width] = useElementWidth<HTMLDivElement>();
  const form = balanceForm(width);
  const net = () =>
    balanceNet({
      hash: activity.txHash,
      participants: activity.participants,
      fee,
      compensation,
      genesis,
    });
  return (
    <DetailSection title="Balance changes">
      <div ref={ref} className="min-w-0">
        {form === 'table' ? (
          <div role="table" aria-label="Balance changes by participant">
            <div
              role="row"
              className={cn(
                BALANCE_GRID,
                'border-base-border-subtle text-text-dim text-meta border-b pb-1.5'
              )}
            >
              <span role="columnheader">Participant</span>
              <span role="columnheader" className="text-right">
                CKB
              </span>
              <span role="columnheader" className="text-right">
                Occupied
              </span>
              <span role="columnheader">Items</span>
            </div>
            {activity.participants.map((p, i) => (
              <div
                key={partyKey(p, i)}
                role="row"
                className={cn(BALANCE_GRID, 'border-base-elevated items-baseline border-b py-2')}
              >
                <span role="cell" className="min-w-0">
                  <ParticipantRefView participant={p} compact quiet />
                  <LockLabel participant={p} lockCodeHashes={lockCodeHashes} className="ml-2" />
                </span>
                <span role="cell" className="text-right">
                  <ExactCkb shannons={BigInt(p.ckbDelta)} />
                </span>
                <span role="cell" className="text-right tabular-nums">
                  {formatOccupiedDelta(p.usedDelta)}
                </span>
                <span role="cell" className="flex min-w-0 flex-wrap gap-x-3.5 gap-y-0.5">
                  <Items participant={p} />
                </span>
              </div>
            ))}
            <div
              role="row"
              data-testid="tx-balance-net"
              className={cn(BALANCE_GRID, 'text-text-dim items-baseline py-2')}
            >
              {readable(activity.txHash, () => {
                const n = net();
                return (
                  <>
                    <span role="cell">Net</span>
                    <span role="cell" className="text-right">
                      <ExactCkb shannons={n.ckb} tone="plain" />
                    </span>
                    <span role="cell" className="text-right tabular-nums">
                      {formatOccupiedDelta(n.occupied.toString())}
                    </span>
                    <span role="cell">{netNote(n.note)}</span>
                  </>
                );
              })}
            </div>
          </div>
        ) : (
          <ul aria-label="Balance changes by participant" data-form={form}>
            {activity.participants.map((p, i) => {
              const party = <ParticipantRefView participant={p} compact quiet />;
              const ckb = <ExactCkb shannons={BigInt(p.ckbDelta)} />;
              const occupied = (
                <>
                  occupied <span className="tabular-nums">{formatOccupiedDelta(p.usedDelta)}</span>
                </>
              );
              const border = i === 0 ? 'border-base-border-subtle' : 'border-base-elevated';
              return form === 'three-line' ? (
                // Phone: who and their lock / how much CKB and occupied / what item moved.
                <li
                  key={partyKey(p, i)}
                  data-balance-card
                  className={cn('flex flex-col gap-1 border-t py-2.5', border)}
                >
                  <div
                    data-line="party"
                    className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-0.5"
                  >
                    {party}
                    <LockLabel participant={p} lockCodeHashes={lockCodeHashes} />
                  </div>
                  <div
                    data-line="amounts"
                    className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-0.5"
                  >
                    <span className="text-body">{ckb}</span>
                    <span className="text-text whitespace-nowrap text-xs">{occupied}</span>
                  </div>
                  {p.itemDeltas.length > 0 && (
                    <div
                      data-line="items"
                      className="flex flex-wrap items-baseline gap-x-3.5 gap-y-0.5 text-xs"
                    >
                      <Items participant={p} />
                    </div>
                  )}
                </li>
              ) : (
                // 480–699 px: party and lock | CKB on the first line, items | occupied on the
                // second.
                <li
                  key={partyKey(p, i)}
                  data-balance-card
                  className={cn(
                    'grid grid-cols-[minmax(0,1fr)_auto] items-baseline gap-x-4 gap-y-1 border-t py-2.5',
                    border
                  )}
                >
                  <div
                    data-role="party"
                    className="flex min-w-0 flex-wrap items-baseline gap-x-2 gap-y-0.5"
                  >
                    {party}
                    <LockLabel participant={p} lockCodeHashes={lockCodeHashes} />
                  </div>
                  <div data-role="ckb" className="text-body text-right">
                    {ckb}
                  </div>
                  <div
                    data-role="items"
                    className="flex min-w-0 flex-wrap items-baseline gap-x-3.5 gap-y-0.5 text-xs"
                  >
                    <Items participant={p} />
                  </div>
                  <div
                    data-role="occupied"
                    className="text-text whitespace-nowrap text-right text-xs"
                  >
                    {occupied}
                  </div>
                </li>
              );
            })}
            <li
              data-testid="tx-balance-net"
              className="border-base-border-subtle text-text-dim flex flex-wrap items-baseline justify-between gap-x-3 gap-y-0.5 border-t pt-2.5 text-xs"
            >
              {readable(activity.txHash, () => {
                const n = net();
                return (
                  <>
                    <span>
                      Net <ExactCkb shannons={n.ckb} tone="plain" /> · occupied{' '}
                      <span className="text-text tabular-nums">
                        {formatOccupiedDelta(n.occupied.toString())}
                      </span>
                    </span>
                    <span>{netNote(n.note)}</span>
                  </>
                );
              })}
            </li>
          </ul>
        )}
      </div>
    </DetailSection>
  );
}
