'use client';
import { ItemDeltaFact, readable } from '@/components/activity-event-row';
import { CkbAmount } from '@/components/ui/ckb-amount';
import { formatOccupiedDelta } from '@/lib/occupied';
import { orderParticipants } from '@/lib/participant-order';
import { ParticipantRefView } from '@/components/participant-ref';
import { LockWordsView, partyLock } from '@/components/ui/lock-kind';
import { DetailSection } from '@/components/ui/detail-layout';
import { useElementWidth } from '@/hooks/useElementWidth';
import type { GlobalActivity, ParticipantInfo } from '@/lib/api';
import { balanceNet } from '@/lib/tx-detail-model';
import { cn } from '@/lib/utils';
import type { ShortSize } from '@/lib/shorten';
import { shortChars, WIDEST_OCCUPIED_CHANGE } from '@/lib/list-bounds';
import { SHORT_FORMS } from '@/lib/shorten';
import { columnPx, fontPx, textPx } from '@/lib/type-metrics';

/** The participant's track and the CKB change's (as they were drawn). */
const PARTY_PX = 190;
const CKB_PX = 180;
/**
 * The Occupied track holds the widest change a party can make in one transaction, with its unit
 * (WIDEST_OCCUPIED_CHANGE, "−8,101,332,271 B"), at the body step its cells inherit from the
 * transaction page: 135 px.
 */
const OCCUPIED_PX = columnPx(textPx(WIDEST_OCCUPIED_CHANGE.length, fontPx('body')));
/**
 * The Items track's floor, a stated room and not a bound (no bound holds what an item writes: a
 * token's amount is a u128, 39 digits, and a symbol or a collection's name is the chain's): its
 * facts wrap between them and between their words, and the room is the widest unit a fact writes
 * that the chain's names do not decide, an object's or an identity's id at the row size,
 * "0x51c7a1b2…96f2ab", 17 glyphs at the body step: 143 px. A wider unit (a long amount, a long
 * name's word) is held on its line and runs past the track into the row's edge.
 */
const ITEMS_MIN_PX = columnPx(textPx(shortChars(SHORT_FORMS.row.hash), fontPx('body')));
/** gap-4, between the four tracks. */
const GAP_PX = 16;
/**
 * The four tracks' minimum and their gaps, 696 px: below it the table would have to scroll, so the
 * cards are drawn. The 768 window's column (736) and 1280's (912) draw the table, 1024's and 640's
 * (608, beside the tiles) the two-line cards, a phone's the three-line cards.
 */
const TABLE_MIN_W = PARTY_PX + CKB_PX + OCCUPIED_PX + ITEMS_MIN_PX + 3 * GAP_PX;
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

const BALANCE_GRID = 'grid gap-4';
/** Each row is its own grid on these tracks (the Items track takes the rest, from its floor). */
const BALANCE_TRACKS = {
  gridTemplateColumns: `${PARTY_PX}px ${CKB_PX}px ${OCCUPIED_PX}px minmax(${ITEMS_MIN_PX}px,1fr)`,
};

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

/**
 * The order a reader needs (lane P1 follow-up; third round, review p-logic-chain NOTE 4): who gave,
 * then who got, then who neither gave nor got; each group in the feed's order, the largest CKB
 * change first (`orderParticipants`). A party gave if its CKB or any item left it: a party that gave
 * only a token stood last, ordered by CKB alone. A party that gave CKB and got an item gave.
 */
function balanceOrder(participants: ParticipantInfo[]): ParticipantInfo[] {
  const gave = (p: ParticipantInfo) =>
    BigInt(p.ckbDelta) < BigInt(0) || p.itemDeltas.some((i) => BigInt(String(i.delta)) < BigInt(0));
  const got = (p: ParticipantInfo) =>
    BigInt(p.ckbDelta) > BigInt(0) || p.itemDeltas.some((i) => BigInt(String(i.delta)) > BigInt(0));
  const ordered = orderParticipants(participants);
  return [
    ...ordered.filter(gave),
    ...ordered.filter((p) => !gave(p) && got(p)),
    ...ordered.filter((p) => !gave(p) && !got(p)),
  ];
}

function partyKey(p: ParticipantInfo, i: number): string {
  return `${p.lockHash ?? p.lockHashPrefix ?? p.address}:${i}`;
}

/**
 * Balance changes (TX-12, TX-19): every participant, then the Net row that explains the sum.
 * The measured width decides the form (docs/RESPONSIVE_LAYOUT.md rule 4): the four-column table
 * where its tracks fit (TABLE_MIN_W, 696 px), otherwise one card per participant — party and lock, CKB and occupied,
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
  // the order a reader needs (lane P1 follow-up; review chain-b): who gave, then who got, each
  // the largest first, as the feed and What moved order them; the Net row, with the fee, last. Read
  // in the section's boundary, so a value it cannot read fails the section in place.
  const parties = () => balanceOrder(activity.participants);
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
                style={BALANCE_TRACKS}
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
              {parties().map((p, i) => (
                <div
                  key={partyKey(p, i)}
                  role="row"
                  style={BALANCE_TRACKS}
                  className={cn(BALANCE_GRID, 'border-row-rule items-baseline border-b py-2')}
                >
                  <span role="cell" className="min-w-0">
                    {/* the row's party, its key (components/ui/link.tsx) */}
                    <ParticipantRefView participant={p} size="row" compact quiet look="key" />
                    {/* a space between the party and its lock: two units, a line between them
                        where they do not fit the track together (B4 second fix round) */}{' '}
                    <LockLabel participant={p} size="row" />
                  </span>
                  <span role="cell" className="text-right">
                    <CkbAmount form="column" shannons={BigInt(p.ckbDelta)} />
                  </span>
                  <span role="cell" className="whitespace-nowrap text-right tabular-nums">
                    {formatOccupiedDelta(
                      p.usedDelta,
                      `transaction ${activity.txHash}, party ${partyKey(p, i)}`
                    )}
                  </span>
                  <span role="cell" className="flex min-w-0 flex-wrap gap-x-3.5 gap-y-0.5">
                    <Items participant={p} size="row" />
                  </span>
                </div>
              ))}
              <div
                role="row"
                data-testid="tx-balance-net"
                style={BALANCE_TRACKS}
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
                        {formatOccupiedDelta(
                          n.occupied.toString(),
                          `transaction ${activity.txHash}, net`
                        )}
                      </span>
                      <span role="cell">{netNote(n.note)}</span>
                    </>
                  );
                })}
              </div>
            </div>
          ) : (
            <ul aria-label="Balance changes by participant" data-form={form}>
              {parties().map((p, i) => {
                // the three-line card is a phone's (under 480 px of container): tight; the
                // two-line card is drawn at 1024 too (the 640 px column), beside the summary,
                // the tiles and What moved, which write the same party at the row size (R9)
                const size: ShortSize = form === 'three-line' ? 'tight' : 'row';
                const party = (
                  <ParticipantRefView participant={p} size={size} compact quiet look="key" />
                );
                const ckb = <CkbAmount form="summary" shannons={BigInt(p.ckbDelta)} />;
                const occupied = (
                  <>
                    occupied{' '}
                    <span className="tabular-nums">
                      {formatOccupiedDelta(
                        p.usedDelta,
                        `transaction ${activity.txHash}, party ${partyKey(p, i)}`
                      )}
                    </span>
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
                          {formatOccupiedDelta(
                            n.occupied.toString(),
                            `transaction ${activity.txHash}, net`
                          )}
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
