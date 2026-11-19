'use client';
import { Fragment, useEffect, useRef, useState, type ReactNode } from 'react';
import {
  FoldedActivityRow,
  rowReadFails,
  type ActivityRowData,
} from '@/components/activity/activity-row';
import { foldRuns, runKey, runParties, type RunSubject } from '@/lib/activity-runs';
import type { ParticipantRef } from '@/lib/api';
import { readable } from '@/components/activity-event-row';

/**
 * What a row's part draws, by which its boundary starts again (B4 third round): its transaction and
 * where it stands, so a row in the pool that commits is drawn anew, never left in an old error.
 */
function versionOf(r: FeedRow): string {
  return `${r.key}@${r.activity.blockNumber ?? 'pool'}`;
}

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
 * What a row is compared on (`lib/activity-runs.ts`): the title in words (`activityTitleText`), its
 * protocol actions and the code of the scripts it called; and its parties, which a run counts (the
 * row's own, unless the feed knows more of them: the address page's other parties). Null for a row
 * that cannot read one of its protocol actions (`rowReadFails`): it reports the error in its place,
 * so it is never folded (lane B4 fix round, review NOTE 1).
 */
export function runSubject(
  activity: ActivityRowData,
  title: string,
  parties: readonly ParticipantRef[] = activity.participants
): RunSubject | null {
  if (rowReadFails(activity)) return null;
  return {
    title,
    txHash: activity.txHash,
    blockNumber: activity.blockNumber,
    protocolActions: activity.protocolActions,
    typeCalls: activity.typeCalls,
    lockCalls: activity.lockCalls,
    parties,
  };
}

/** A run's row's subject: a row in a run of two or more was compared, so it has one. */
function subjectOf(r: FeedRow): RunSubject {
  if (r.subject === null) throw new Error(`transaction ${r.key}: a row in a run has no subject`);
  return r.subject;
}

/**
 * A feed's rows with its runs folded (readability lane B4, §7 point 6; the rule is
 * `lib/activity-runs.ts`, the owner's of Gate B): a run of two or more rows that did the same thing
 * and called the same script code is one `FoldedActivityRow`, opened in place to the ordinary rows;
 * every other row is itself. A run folds within the rows given: a paged feed folds within its page.
 *
 * A live feed adds rows above (a poll) and below (an older page) and never remounts or folds what
 * the reader has in front of them (lane B4 fix round):
 *
 * - each part is keyed by what it is, in one flat list: a row by its transaction (`key`), so a row
 *   that joins a run or leaves one keeps its own state; a run's head by the first of its rows it was
 *   drawn with, which a row arriving above or below it does not change, so its toggle keeps the
 *   focus;
 * - a run is open while any of its rows is one the reader has before them whole: in a run they
 *   opened, or drawn alone. A row arriving above or below an open run joins it open; a row drawn
 *   whole that an older page's first row continues starts a run drawn open, never folded under the
 *   reader; a folded run takes the rows in and stays folded. Folding a run puts all its rows away.
 */
export function ActivityRuns({
  rows,
  age = false,
  showScriptDetails = true,
}: {
  rows: readonly FeedRow[];
  age?: boolean;
  showScriptDetails?: boolean;
}) {
  // the rows of the runs the reader opened, by their keys
  const [opened, setOpened] = useState<ReadonlySet<string>>(() => new Set());
  // the rows drawn alone, whole, as the last render committed them
  const drawn = useRef(new Set<string>());
  // a row's run's head, by the row's key: the first of its rows the run was drawn with
  const heads = useRef(new Map<string, string>());
  const taken = new Set<string>();
  const runs = foldRuns(rows, (r) =>
    r.subject === null ? JSON.stringify(['unread', r.key]) : runKey(r.subject)
  ).map((run) => {
    if (run.length === 1) return { run, head: null, isOpen: true };
    const head =
      run.map((r) => heads.current.get(r.key)).find((h) => h !== undefined && !taken.has(h)) ??
      run[0].key;
    taken.add(head);
    const isOpen = run.some((r) => opened.has(r.key) || drawn.current.has(r.key));
    return { run, head, isOpen };
  });
  useEffect(() => {
    for (const { run, head } of runs) {
      if (head === null) drawn.current.add(run[0].key);
      else for (const r of run) heads.current.set(r.key, head);
    }
  });
  const toggle = (run: readonly FeedRow[], isOpen: boolean) => {
    if (isOpen) for (const r of run) drawn.current.delete(r.key);
    setOpened((was) => {
      const next = new Set(was);
      for (const r of run) {
        if (isOpen) next.delete(r.key);
        else next.add(r.key);
      }
      return next;
    });
  };
  return (
    <>
      {runs.flatMap(({ run, head, isOpen }) => {
        const [lead] = run;
        // each row, and each run's head, fails in its place (ruling D7): what a row cannot read
        // (a lock the decision refuses) is said where the row stands, the others drawn
        if (head === null)
          return [
            <Fragment key={lead.key}>
              {readable(lead.key, () => lead.row, versionOf(lead))}
            </Fragment>,
          ];
        const folded = (
          <FoldedActivityRow
            rows={run.map((r) => r.activity)}
            participants={runParties(run.map(subjectOf))}
            title={lead.title}
            showScriptDetails={showScriptDetails}
            age={age}
            className={lead.className}
            fresh={run.some((r) => r.fresh)}
            open={isOpen}
            onToggle={() => toggle(run, isOpen)}
          />
        );
        const headed = <Fragment key={`run:${head}`}>{readable(head, () => folded)}</Fragment>;
        return isOpen
          ? [
              headed,
              ...run.map((r) => (
                <Fragment key={r.key}>{readable(r.key, () => r.row, versionOf(r))}</Fragment>
              )),
            ]
          : [headed];
      })}
    </>
  );
}
