import type { ReactNode } from 'react';
import { PhoneRow } from '@/components/ui/phone-row';
import {
  PHONE_SHARE_TRACK,
  ShareAxis,
  ShareBar,
  WIDE_SHARE_TRACK,
} from '@/components/ui/share-bar';

/** The bar's own number beside its track (a share): one fixed width, so it starts at one x too. */
const SHARE_TEXT = 'inline-block w-[7ch] text-right';

/**
 * A holder on a phone (board L5): rank, holder, lock and amount on one line; the bar on the list's
 * one scale (ruling R7: the table's rounded maximum, marked on the first row's axis) with its
 * number beside it and any quiet fact at the end, on the second. Token holders, collection holders
 * and DAO depositors share it. In a wide container (the 640 band, where a table does not fit) one
 * line: rank · holder · lock · fact · amount · bar · share, the bar ending at one x.
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
  wide?: boolean;
}) {
  const holder = (
    <>
      <span data-role="rank" className="text-text-dim inline-block w-[22px]">
        {rank.toLocaleString('en-US')}
      </span>
      {party}
      {lock !== undefined && lock !== null && lock !== false && (
        <>
          {' '}
          <span className="text-text-dim text-small whitespace-nowrap">{lock}</span>
        </>
      )}
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
      lines={[
        { key: 'holder', tone: 'head', left: holder, right: figure },
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
          right: note,
        },
      ]}
    />
  );
}
