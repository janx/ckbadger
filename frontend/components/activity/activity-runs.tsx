'use client';
import { Fragment, useState, type ReactNode } from 'react';
import { FoldedActivityRow, type ActivityRowData } from '@/components/activity/activity-row';
import { foldRuns, runKey, type RunSubject } from '@/lib/activity-runs';
import type { ParticipantRef } from '@/lib/api';

/** One row of a feed, with what its run is drawn and compared from. */
export interface FeedRow {
  /** The row's transaction hash: its React key, and its run's when it leads one. */
  key: string;
  /**
   * What the row is compared with its neighbours on (`lib/activity-runs.ts`); null for a row the
   * feed could not read, which is never folded and reports itself in its place.
   */
  subject: RunSubject | null;
  activity: ActivityRowData;
  /** Its title as its head draws it (`ActivityTitle`), for the folded run's head. */
  title: ReactNode;
  /** The class the feed gives the row (`ActivityRow`'s), given to its run too (a set-back row). */
  className?: string;
  /** Just arrived (a live feed's highlight). */
  fresh?: boolean;
  /** The ordinary row. */
  row: ReactNode;
}

/**
 * What a row is compared on: the title in words (`activityTitleText`), its scripts and its parties
 * (the row's own, unless the feed knows more of them: the address page's other parties).
 */
export function runSubject(
  activity: ActivityRowData,
  title: string,
  parties: readonly ParticipantRef[] = activity.participants
): RunSubject {
  return {
    title,
    txHash: activity.txHash,
    blockNumber: activity.blockNumber,
    typeCalls: activity.typeCalls,
    lockCalls: activity.lockCalls,
    parties,
  };
}

/**
 * A feed's rows with its runs folded (readability lane B4, §7 point 6; the rule is
 * `lib/activity-runs.ts`): a run of two or more rows that say the same thing is one
 * `FoldedActivityRow`, opened in place to the ordinary rows; every other row is itself. A run
 * folds within the rows given: a paged feed folds within its page.
 */
export function ActivityRuns({
  rows,
  age = false,
  showScriptDetails = true,
  focusLockHash,
}: {
  rows: readonly FeedRow[];
  age?: boolean;
  showScriptDetails?: boolean;
  focusLockHash?: string;
}) {
  // which runs are open, by the key of the row that leads each
  const [open, setOpen] = useState<ReadonlySet<string>>(() => new Set());
  const runs = foldRuns(rows, (r) =>
    r.subject === null ? JSON.stringify(['unread', r.key]) : runKey(r.subject)
  );
  const toggle = (key: string) =>
    setOpen((was) => {
      const next = new Set(was);
      if (!next.delete(key)) next.add(key);
      return next;
    });
  return (
    <>
      {runs.map((run) => {
        const [lead] = run;
        if (run.length === 1) return <Fragment key={lead.key}>{lead.row}</Fragment>;
        const isOpen = open.has(lead.key);
        return (
          <Fragment key={`run:${lead.key}`}>
            <FoldedActivityRow
              rows={run.map((r) => r.activity)}
              title={lead.title}
              showScriptDetails={showScriptDetails}
              age={age}
              focusLockHash={focusLockHash}
              className={lead.className}
              fresh={run.some((r) => r.fresh)}
              open={isOpen}
              onToggle={() => toggle(lead.key)}
            />
            {isOpen && run.map((r) => <Fragment key={r.key}>{r.row}</Fragment>)}
          </Fragment>
        );
      })}
    </>
  );
}
