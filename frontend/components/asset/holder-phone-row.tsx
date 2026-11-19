import type { ReactNode } from 'react';
import { Dotted, PhoneRow } from '@/components/ui/phone-row';
import {
  PHONE_SHARE_TRACK,
  ShareAxis,
  ShareBar,
  WIDE_SHARE_TRACK,
} from '@/components/ui/share-bar';

/** The bar's own number beside its track (a share): one fixed width, so it starts at one x too. */
const SHARE_TEXT = 'inline-block w-[7ch] text-right';

/**
 * A holder on a phone (board L5, the phone-rows board; PB-09): the rank, the holder and the amount,
 * the figure the list sorts by, on line 1; the bar on the list's one scale (ruling R7: the table's
 * rounded maximum, marked on the first row's axis) with its number beside it, and at that line's
 * right edge, held there, the holder's lock where it is not the default lock and any quiet fact. The
 * lock stood on line 1 after the address, where a long one dropped the amount under it: one list
 * took two shapes by the length of its strings. Token holders, collection holders and DAO
 * depositors share it, each stating its first line (`head`). In a wide container (the 640 band,
 * where a table does not fit) one line: rank · holder · lock · fact · amount · bar · share, the bar
 * ending at one x.
 */
export function HolderPhoneRow({
  rank,
  party,
  lock,
  amount,
  share,
  note,
  bar,
  axis,
  head,
  wide = false,
}: {
  rank: number;
  party: ReactNode;
  /** The holder's lock kind; DAO depositors have none of their own to show. */
  lock?: ReactNode;
  amount: ReactNode;
  /** The bar's exact number ("52.73%"); a list of counts prints its count as the amount instead. */
  share?: ReactNode;
  /** A quiet fact that goes with the holder ("3 deposits"). */
  note?: ReactNode;
  /** The holder's value on the list's scale (the table's), in the class colour. */
  bar: { value: number; scale: number; color: string };
  /** The scale's end as its axis mark ("60%"): the list's first row carries it over its track. */
  axis?: string;
  /** Line 1's width as its list states it (PhoneRow's `head`). */
  head: number;
  wide?: boolean;
}) {
  const hasLock = lock !== undefined && lock !== null && lock !== false;
  // the rank's box: 22 px holds two digits at the key's 14 px (16.8) and a 4 px gap; a longer rank
  // widens it, never running into the holder (a fixed 22 px box let "100" overlap the address)
  const rankBox = (
    <span data-role="rank" className="text-text-dim inline-block min-w-[22px] pr-1">
      {rank.toLocaleString('en-US')}
    </span>
  );
  const lockWords = hasLock && (
    <span className="text-text-dim text-small whitespace-nowrap">{lock}</span>
  );
  const holder = (
    <>
      {rankBox}
      {party}
      {lockWords && <> {lockWords}</>}
    </>
  );
  const figure = <span className="text-text-bright text-body">{amount}</span>;
  const shareText = share !== undefined && share !== null && (
    <span className={`text-text-dim text-meta ${SHARE_TEXT}`}>{share}</span>
  );
  if (wide)
    return (
      <PhoneRow
        testId="holder-row-mobile"
        head={head}
        lines={[
          axis !== undefined && {
            key: 'axis',
            tone: 'quiet',
            left: null,
            right: (
              <span className="inline-flex items-center gap-3">
                <ShareAxis max={axis} track={WIDE_SHARE_TRACK} />
                {shareText && <span className={SHARE_TEXT} />}
              </span>
            ),
          },
          {
            key: 'holder',
            tone: 'head',
            left: holder,
            right: (
              <span className="inline-flex items-center gap-3">
                {note !== undefined && <span className="text-text-dim text-meta">{note}</span>}
                {figure}
                <ShareBar {...bar} track={WIDE_SHARE_TRACK} />
                {shareText}
              </span>
            ),
          },
        ]}
      />
    );
  return (
    <PhoneRow
      testId="holder-row-mobile"
      head={head}
      lines={[
        {
          key: 'holder',
          tone: 'head',
          left: (
            <>
              {rankBox}
              {party}
            </>
          ),
          right: figure,
        },
        axis !== undefined && {
          key: 'axis',
          tone: 'quiet',
          left: (
            <span className="ml-[22px] inline-flex">
              <ShareAxis max={axis} track={PHONE_SHARE_TRACK} />
            </span>
          ),
        },
        {
          key: 'share',
          tone: 'quiet',
          left: (
            <span className="ml-[22px] inline-flex items-center gap-2.5">
              <ShareBar {...bar} track={PHONE_SHARE_TRACK} />
              {shareText}
            </span>
          ),
          // held at the line's right edge: where the lock and the fact stand never depends on how
          // long the share's text is
          hold: true,
          right: (hasLock || note !== undefined) && <Dotted parts={[lockWords, note]} />,
        },
      ]}
    />
  );
}
