'use client';
import { ItemDeltaFact, readable } from '@/components/activity-event-row';
import { CkbAmount } from '@/components/ui/ckb-amount';
import { formatOccupiedDelta } from '@/lib/occupied';
import { ParticipantRefView } from '@/components/participant-ref';
import { LockWordsView, partyLock } from '@/components/ui/lock-kind';
import { DetailSection } from '@/components/ui/detail-layout';
import { useElementWidth } from '@/hooks/useElementWidth';
import type { GlobalActivity, ParticipantInfo } from '@/lib/api';
import { balanceNet } from '@/lib/tx-detail-model';
import { cn } from '@/lib/utils';
import type { ShortSize } from '@/lib/shorten';

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
 * A party's lock beside it, as every row writes one (the one decision, `partyLock`: the lock read
 * off the party's address; nothing for the default lock), at the form's size (U-09).
 */
function LockLabel({
  participant,
  size,
  className,
}: {
  participant: ParticipantInfo;
  size: ShortSize;
  className?: string;
}) {
  const words = partyLock(participant);
  if (words === null) return null;
  return (
    <span data-role="lock" className={className}>
      <LockWordsView
        words={words}
        size={size}
        className="text-text-dim text-meta whitespace-nowrap"
      />
    </span>
  );
}

function Items({ participant, size }: { participant: ParticipantInfo; size: ShortSize }) {
  return (
    <>
      {participant.itemDeltas.map((item, j) => (
        <ItemDeltaFact key={j} item={item} size={size} />
      ))}
    </>
  );
}

const BALANCE_GRID = 'grid grid-cols-[190px_180px_100px_minmax(0,1fr)] gap-4';

function netNote(note: ReturnType<typeof balanceNet>['note']): string {
  switch (note) {
    case 'fee':
      return 'the fee, paid to the miner';
    case 'no-fee':
      return 'no fee: nothing paid to the miner';
    case 'compensation':
      return 'Nervos DAO compensation, less the fee';
  }
}

function partyKey(p: ParticipantInfo, i: number): string {
  return `${p.lockHash ?? p.lockHashPrefix ?? p.address}:${i}`;
}

/**
 * Balance changes (TX-12, TX-19): every participant, then the Net row that explains the sum.
 * The measured width decides the form (docs/RESPONSIVE_LAYOUT.md rule 4): the four-column table
 * where its 700 px fit, otherwise one card per participant — party and lock, CKB and occupied,
 * the items moved — with the Net row as the last card. Never a sideways scroller. A value the
 * section cannot read fails the section in place, under its heading (review 45fd ruling D7).
 */
export function BalanceChanges({
  activity,
  fee,
  compensation,
}: {
  activity: GlobalActivity;
  fee: string;
  compensation: bigint | null;
}) {
  const [ref, width] = useElementWidth<HTMLDivElement>();
  const form = balanceForm(width);
  const net = () =>
    balanceNet({
      hash: activity.txHash,
      participants: activity.participants,
      fee,
      compensation,
    });
  return (
    <DetailSection title="Balance changes">
      {readable(activity.txHash, () => (
        <div ref={ref} className="min-w-0">
          {form === 'table' ? (
            <div role="table" aria-label="Balance changes by participant">
              <div
                role="row"
                className={cn(
                  BALANCE_GRID,
                  'border-row-rule text-text-dim text-meta border-b pb-1.5'
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
                  className={cn(BALANCE_GRID, 'border-row-rule items-baseline border-b py-2')}
                >
                  <span role="cell" className="min-w-0">
                    <ParticipantRefView participant={p} size="row" compact quiet />
                    <LockLabel participant={p} size="row" className="ml-2" />
                  </span>
                  <span role="cell" className="text-right">
                    <CkbAmount form="column" shannons={BigInt(p.ckbDelta)} />
                  </span>
                  <span role="cell" className="whitespace-nowrap text-right tabular-nums">
                    {formatOccupiedDelta(p.usedDelta)}
                  </span>
                  <span role="cell" className="flex min-w-0 flex-wrap gap-x-3.5 gap-y-0.5">
                    <Items participant={p} size="row" />
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
                        <CkbAmount form="column" shannons={n.ckb} tone="plain" />
                      </span>
                      <span role="cell" className="whitespace-nowrap text-right tabular-nums">
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
                // the three-line card is a phone's (under 480 px of container): tight; the
                // two-line card is drawn at 1024 too (the 640 px column), beside the summary,
                // the tiles and What moved, which write the same party at the row size (R9)
                const size: ShortSize = form === 'three-line' ? 'tight' : 'row';
                const party = <ParticipantRefView participant={p} size={size} compact quiet />;
                const ckb = <CkbAmount form="summary" shannons={BigInt(p.ckbDelta)} />;
                const occupied = (
                  <>
                    occupied{' '}
                    <span className="tabular-nums">{formatOccupiedDelta(p.usedDelta)}</span>
                  </>
                );
                return form === 'three-line' ? (
                  // Phone: who and their lock / how much CKB and occupied / what item moved.
                  <li
                    key={partyKey(p, i)}
                    data-balance-card
                    className="border-row-rule flex flex-col gap-1 border-t py-2.5"
                  >
                    <div
                      data-line="party"
                      className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-0.5"
                    >
                      {party}
                      <LockLabel participant={p} size={size} />
                    </div>
                    <div
                      data-line="amounts"
                      className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-0.5"
                    >
                      <span className="text-body">{ckb}</span>
                      <span className="text-text text-small whitespace-nowrap">{occupied}</span>
                    </div>
                    {p.itemDeltas.length > 0 && (
                      <div
                        data-line="items"
                        className="text-small flex flex-wrap items-baseline gap-x-3.5 gap-y-0.5"
                      >
                        <Items participant={p} size={size} />
                      </div>
                    )}
                  </li>
                ) : (
                  // 480–699 px: party and lock | CKB on the first line, items | occupied on the
                  // second.
                  <li
                    key={partyKey(p, i)}
                    data-balance-card
                    className="border-row-rule grid grid-cols-[minmax(0,1fr)_auto] items-baseline gap-x-4 gap-y-1 border-t py-2.5"
                  >
                    <div
                      data-role="party"
                      className="flex min-w-0 flex-wrap items-baseline gap-x-2 gap-y-0.5"
                    >
                      {party}
                      <LockLabel participant={p} size={size} />
                    </div>
                    <div data-role="ckb" className="text-body text-right">
                      {ckb}
                    </div>
                    <div
                      data-role="items"
                      className="text-small flex min-w-0 flex-wrap items-baseline gap-x-3.5 gap-y-0.5"
                    >
                      <Items participant={p} size={size} />
                    </div>
                    <div
                      data-role="occupied"
                      className="text-text text-small whitespace-nowrap text-right"
                    >
                      {occupied}
                    </div>
                  </li>
                );
              })}
              <li
                data-testid="tx-balance-net"
                className="border-row-rule text-text-dim text-small flex flex-wrap items-baseline justify-between gap-x-3 gap-y-0.5 border-t pt-2.5"
              >
                {readable(activity.txHash, () => {
                  const n = net();
                  return (
                    <>
                      <span>
                        Net <CkbAmount form="summary" shannons={n.ckb} tone="plain" /> · occupied{' '}
                        <span className="text-text whitespace-nowrap tabular-nums">
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
      ))}
    </DetailSection>
  );
}
