import type { ReactNode } from 'react';
import { PhoneRow } from '@/components/ui/phone-row';
import { ShareBar } from '@/components/ui/share-bar';

/**
 * A holder on a phone (board L5): rank, holder, lock and amount on one line; the share as a bar
 * (10 % = a full bar) with its number on the second. Token holders, collection holders and DAO
 * depositors share it. In a wide container (the 640 band, where a table does not fit) one line:
 * rank · holder · lock · bar · share · amount.
 */
export function HolderPhoneRow({
  rank,
  party,
  lock,
  amount,
  share,
  barWidth,
  barColor,
  wide = false,
}: {
  rank: number;
  party: ReactNode;
  /** The holder's lock kind; DAO depositors have none of their own to show. */
  lock?: ReactNode;
  amount: ReactNode;
  /** The share's exact number and any quiet fact that goes with it ("1.00% · 3 deposits"). */
  share?: ReactNode;
  barWidth: string;
  barColor?: string;
  wide?: boolean;
}) {
  const holder = (
    <>
      <span data-role="rank" className="text-text-dim inline-block w-[22px]">
        {rank.toLocaleString('en-US')}
      </span>
      {party}
      {lock !== undefined && (
        <>
          {' '}
          <span className="text-text-dim whitespace-nowrap text-xs">{lock}</span>
        </>
      )}
    </>
  );
  const figure = <span className="text-text-bright text-body">{amount}</span>;
  if (wide)
    return (
      <PhoneRow
        testId="holder-row-mobile"
        lines={[
          {
            key: 'holder',
            tone: 'head',
            left: holder,
            right: (
              <span className="inline-flex items-center gap-3">
                <ShareBar width={barWidth} color={barColor} className="w-16 flex-none" />
                {share !== undefined && share !== null && (
                  <span className="text-text-dim text-meta">{share}</span>
                )}
                {figure}
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
        {
          key: 'share',
          tone: 'quiet',
          left: <ShareBar width={barWidth} color={barColor} className="ml-[22px]" />,
          right: share,
          grow: true,
        },
      ]}
    />
  );
}
