'use client';
import { Fragment, useEffect, useId, useRef, useState, type ReactNode } from 'react';
import {
  FoldedActivityRow,
  rowReadFails,
  type ActivityRowData,
} from '@/components/activity/activity-row';
import { foldRuns, runKey, runSenders, type RunSubject } from '@/lib/activity-runs';
import type { ParticipantRef } from '@/lib/api';
import { readable } from '@/components/activity-event-row';
import { sentField } from '@/lib/api-fields';

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
 * protocol actions and the code its items carry and its scripts called; and its parties, which a
 * run counts (the row's own, unless the feed knows more of them: the address page's other
 * parties). Null for a row that cannot read one of its protocol actions (`rowReadFails`): it
 * reports the error in its place, so it is never folded (lane B4 fix round, review NOTE 1). Every
 * other row has one, a committed row and a pending row alike: its recognised codes are read from
 * its row (`recognizedTypes`, C7), never resolved per item, so no row stands alone for codes it
 * cannot say (a pending row folds with nothing anyway). A row that states no list is refused: an
 * API older than the page.
 */
export function runSubject(
  activity: ActivityRowData,
  title: string,
  parties: readonly ParticipantRef[] = activity.participants
): RunSubject | null {
  if (rowReadFails(activity)) return null;
  // the API states the list on every row (empty where no recognised code ran): a row without it
  // is older than this page, refused by name through the one reader; a row whose list is no list
  // is not this API either
  const recognizedTypes = sentField(
    activity.recognizedTypes,
    `transaction ${activity.txHash}`,
    'recognizedTypes'
  );
  if (!Array.isArray(recognizedTypes))
    throw new Error(
      `transaction ${activity.txHash}: the row states recognizedTypes ${String(recognizedTypes)}, not a list`
    );
  return {
    title,
    txHash: activity.txHash,
    blockNumber: activity.blockNumber,
    isCellbase: activity.isCellbase,
    protocolActions: activity.protocolActions,
    typeCalls: activity.typeCalls,
    lockCalls: activity.lockCalls,
    recognizedTypes,
    parties,
  };
}

/**
 * What the runs carry to the next render (`ActivityRuns`), bound to the rows in the feed now (the
 * third read's Lb NOTE 9: it kept every row it ever drew, for the feed's life, the shape the stream's
 * day anchors were bound from in 2e): the rows drawn alone, whole (`drawn`, of the last render's,
 * those still in the feed, and this render's lone rows), and each row's run head (`heads`, this
 * render's runs). A row that left the feed is forgotten.
 */
export function runMemory(
  runs: readonly { run: readonly { key: string }[]; head: string | null }[],
  drawn: ReadonlySet<string>
): { drawn: Set<string>; heads: Map<string, string> } {
  const present = new Set(runs.flatMap(({ run }) => run.map((r) => r.key)));
  const next = new Set([...drawn].filter((key) => present.has(key)));
  const heads = new Map<string, string>();
  for (const { run, head } of runs) {
    if (head === null) next.add(run[0].key);
    else for (const r of run) heads.set(r.key, head);
  }
  return { drawn: next, heads };
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
  more,
  age = false,
  showScriptDetails = true,
}: {
  rows: readonly FeedRow[];
  /**
   * Whether rows older than the last given are unread (a next page, the stream's older pages): the
   * last run of the rows given is then not final, the next row may continue it, and its counts
   * say "so far" (the lead's finding, the exactness lane's B9). Every feed that folds says it.
   */
  more: boolean;
  age?: boolean;
  showScriptDetails?: boolean;
}) {
  // the list the rows stand in, which each run's toggle names (aria-controls)
  const listId = useId();
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
    const memory = runMemory(runs, drawn.current);
    drawn.current = memory.drawn;
    heads.current = memory.heads;
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
    // one element in every state, so no row remounts as runs open and fold (a block, as the rows)
    <div id={listId} data-role="feed-rows">
      {runs.flatMap(({ run, head, isOpen }, at) => {
        const [lead] = run;
        // each row, and each run's head, fails in its place (ruling D7): what a row cannot read
        // (a lock the decision refuses) is said where the row stands, the others drawn
        if (head === null)
          return [
            <Fragment key={lead.key}>
              {readable(lead.key, () => lead.row, versionOf(lead))}
            </Fragment>,
          ];
        // drawn inside the run's boundary, so a run that cannot be counted (no party of a
        // transaction sent) is refused in its place, as a row is
        const folded = () => (
          <FoldedActivityRow
            rows={run.map((r) => r.activity)}
            // a run of cellbases has no sender to count (none of them has an input)
            senders={lead.activity.isCellbase ? null : runSenders(run.map(subjectOf))}
            title={lead.title}
            showScriptDetails={showScriptDetails}
            age={age}
            className={lead.className}
            fresh={run.some((r) => r.fresh)}
            open={isOpen}
            final={!(more && at === runs.length - 1)}
            controls={listId}
            onToggle={() => toggle(run, isOpen)}
          />
        );
        const headed = <Fragment key={`run:${head}`}>{readable(head, folded)}</Fragment>;
        return isOpen
          ? [
              headed,
              ...run.map((r) => (
                <Fragment key={r.key}>{readable(r.key, () => r.row, versionOf(r))}</Fragment>
              )),
            ]
          : [headed];
      })}
    </div>
  );
}
