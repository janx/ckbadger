'use client';

import { useState } from 'react';
import { useElementWidth } from '@/hooks/useElementWidth';
import type { ScriptCustodyApi, ScriptFamilyDetail, ScriptHistoryMarkerApi } from '@/lib/api';
import { formatCount } from '@/lib/format-count';
import { holderName, shortHex, signingText, type CodeHolder } from '@/lib/script-lineage';
import { cn, formatUtc8Date } from '@/lib/utils';

/**
 * "Who can change its code" as a picture (table-rows plan L1-3, rule T8; board ScriptDetail): one
 * 28 px bar from the first deployment to today, a segment per holder of the code cell, sized by how
 * long it held it, its name written inside where it fits; the version days marked above; one
 * reading row (the holder now, or the hovered / tapped one) and one caption under it. Every fact the
 * former three columns of sentences wrote — signing, versions signed, keys, keys kept across a
 * handover — is the segment's title.
 */
export interface HolderTimelineProps {
  family: Pick<ScriptFamilyDetail, 'name' | 'versionsCount'>;
  /** The deployed markers of the family's one Type ID reference, oldest first (`typeLineage`). */
  lineage: readonly ScriptHistoryMarkerApi[];
  /** The lineage's code-cell holders, oldest first (`codeHolders`). */
  holders: readonly CodeHolder[];
  numbers: ReadonlyMap<string, number>;
  /** Today, ms: the bar's right end. */
  now: number;
}

/** Past holders alternate two greys; the holder now is the lightest, with the left rule. */
const PAST_FILL = ['#222840', '#2a3146'] as const;
const CURRENT_FILL = '#3a4258';
/** Monospace advance in px at a font size: whole pixels where Chromium snaps them. */
const advance = (px: number) => Math.max(Math.round(0.6 * px), 0.6 * px);
/** A segment's words: 12 px with 6 px each side. */
const LABEL_PX = 12;
const LABEL_PAD = 6;
/** A version tick's words: 11 px, 4 px after the tick, 8 px apart from the next. */
const TICK_PX = 11;
const TICK_PAD = 4;
const TICK_GAP = 8;
const TICK_ROW_PX = 14;

function day(ms: number): string {
  return formatUtc8Date(ms);
}

/** `multisig 0xd1dbde…b95c`: the holder's name with a longer hex, for the title. */
function longName(custody: ScriptCustodyApi): string {
  if ((custody.class === 'multisig' || custody.class === 'one_key') && custody.lockArgs)
    return `${custody.class === 'multisig' ? 'multisig' : 'one key'} ${shortHex(custody.lockArgs, 6, 4)}`;
  return holderName(custody);
}

function versionList(signed: readonly (number | null)[]): string {
  const names = signed.map((n) => (n === null ? 'an unresolved version' : `v${n}`));
  if (names.length <= 1) return names.join('');
  return `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`;
}

/** "signed v2 and v3", "has never signed" or "no upgrade signed". */
function signedText(holder: CodeHolder): string {
  if (holder.signed.length) return `signed ${versionList(holder.signed)}`;
  return holder.neverSigned ? 'has never signed' : 'no upgrade signed';
}

/** The signer state as the reading writes it: "3 of 5 signatures · signed v2 and v3". */
function signerState(holder: CodeHolder): string {
  return [signingText(holder), signedText(holder)].filter(Boolean).join(' · ');
}

function holderDates(holder: CodeHolder): string {
  return holder.until
    ? `${day(holder.from.timestamp)} → ${day(holder.until.timestamp)}`
    : `since ${day(holder.from.timestamp)}`;
}

/** Everything known about one holder, for its segment's title. */
function holderTitle(holder: CodeHolder, previous: CodeHolder | null): string {
  const keys = holder.signing?.keyHashes ?? [];
  const before = previous?.signing?.keyHashes ?? [];
  return [
    longName(holder.custody),
    signingText(holder),
    signedText(holder),
    holderDates(holder),
    `lock args ${holder.lockArgs}`,
    keys.length
      ? `keys ${keys.join(', ')}`
      : holder.neverSigned
        ? 'its keys and threshold appear on chain the first time it signs'
        : '',
    previous && keys.length && before.length
      ? `kept ${keys.filter((key) => before.includes(key)).length} of ${before.length} keys of ${holderName(previous.custody)} at the handover on ${day(holder.from.timestamp)}`
      : '',
  ]
    .filter(Boolean)
    .join(' · ');
}

/** The longest of a segment's names that fits `px`: full, the name, the lock args, or nothing. */
function segmentLabel(holder: CodeHolder, current: boolean, px: number | null): string {
  if (px === null) return '';
  const name = holderName(holder.custody);
  const signing = signingText(holder);
  // A keyed holder (multisig, one key) shortens to its lock args; any other is its name or nothing.
  const keyed =
    (holder.custody.class === 'multisig' || holder.custody.class === 'one_key') &&
    holder.custody.lockArgs !== null &&
    holder.custody.lockArgs.length > 2;
  const candidates = [
    current ? `${name} · holds the code now` : signing ? `${name} · ${signing}` : name,
    name,
    keyed ? shortHex(holder.custody.lockArgs!) : '',
  ];
  return (
    candidates.find((text) => text && text.length * advance(LABEL_PX) + 2 * LABEL_PAD <= px) ?? ''
  );
}

interface Tick {
  key: string;
  versions: string;
  date: string;
  at: number;
  row: number;
  anchorRight: boolean;
}

/**
 * The version days above the bar: "v1 · v2 2023-10-07" at the first deployment of that day, on a
 * second row where it would touch the label before it; hung from its tick near the right end.
 */
function versionTicks(
  lineage: readonly ScriptHistoryMarkerApi[],
  numbers: ReadonlyMap<string, number>,
  position: (ms: number) => number,
  width: number | null
): Tick[] {
  const days = new Map<string, { at: number; versions: string[] }>();
  for (const marker of lineage) {
    const date = day(marker.timestamp);
    const n = marker.versionHash ? numbers.get(marker.versionHash) : undefined;
    if (marker.versionHash && n === undefined)
      throw new Error(
        `Deployment ${marker.codeCellTxHash} names version ${marker.versionHash}, which the family does not list`
      );
    const entry = days.get(date) ?? { at: position(marker.timestamp), versions: [] };
    entry.versions.push(n === undefined ? 'unresolved' : `v${n}`);
    days.set(date, entry);
  }
  const ends = [-Infinity, -Infinity];
  return [...days].map(([date, { at, versions }]) => {
    const text = `${versions.join(' · ')} ${date}`;
    if (width === null)
      return { key: date, versions: versions.join(' · '), date, at, row: 0, anchorRight: false };
    const w = text.length * advance(TICK_PX) + TICK_PAD;
    const x = (at / 100) * width;
    const anchorRight = x + w > width;
    const [start, end] = anchorRight ? [x - w, x] : [x, x + w];
    const row = start >= ends[0] + TICK_GAP ? 0 : start >= ends[1] + TICK_GAP ? 1 : 0;
    ends[row] = end;
    return { key: date, versions: versions.join(' · '), date, at, row, anchorRight };
  });
}

export function HolderTimeline({ family, lineage, holders, numbers, now }: HolderTimelineProps) {
  const [ref, width] = useElementWidth<HTMLDivElement>();
  const [active, setActive] = useState<number | null>(null);
  if (!holders.length) throw new Error(`${family.name}: a holder timeline needs a holder`);
  const start = holders[0].from.timestamp;
  const last = holders[holders.length - 1];
  // Today, or the last handover where this machine's clock is behind the chain.
  const end = Math.max(now, last.from.timestamp);
  const span = Math.max(end - start, 1);
  const position = (ms: number) => ((ms - start) / span) * 100;
  const share = (holder: CodeHolder) =>
    position(holder.until ? holder.until.timestamp : end) - position(holder.from.timestamp);
  const ticks = versionTicks(lineage, numbers, position, width);
  const rows = Math.max(...ticks.map((t) => t.row), 0) + 1;
  const described = holders
    .map((holder, i) => {
      const name = holderName(holder.custody);
      if (!holder.until)
        return i === 0 ? `${name} since ${day(holder.from.timestamp)}` : `${name} since then`;
      return i === 0
        ? `${name} from ${day(holder.from.timestamp)} to ${day(holder.until.timestamp)}`
        : `${name} to ${day(holder.until.timestamp)}`;
    })
    .join(', ');
  const shown = active === null ? holders.length - 1 : active;
  const reading = holders[shown];
  const isNow = reading.until === null;
  return (
    <figure data-testid="holder-timeline" className="min-w-0">
      <div
        aria-hidden="true"
        className="text-meta text-text-dim relative"
        style={{ height: rows * TICK_ROW_PX + 4 }}
      >
        {ticks.map((tick) => (
          <span
            key={tick.key}
            data-version-tick={tick.date}
            data-row={tick.row}
            className={cn(
              'absolute whitespace-nowrap border-[#4b5266] leading-[14px]',
              tick.anchorRight ? 'border-r pr-1' : 'border-l pl-1'
            )}
            style={{
              top: tick.row * TICK_ROW_PX,
              ...(tick.anchorRight ? { right: `${100 - tick.at}%` } : { left: `${tick.at}%` }),
            }}
          >
            {tick.versions} <span className="text-[#4b5266]">{tick.date}</span>
          </span>
        ))}
      </div>
      <div
        ref={ref}
        role="group"
        aria-label={`Who held ${family.name}'s code cell over time: ${described}`}
        className="flex h-[28px] w-full min-w-0 gap-[2px]"
      >
        {holders.map((holder, i) => {
          const current = holder.until === null;
          const px = width === null ? null : (width * share(holder)) / 100;
          return (
            <button
              key={`${holder.lockArgs}:${holder.from.codeCellTxHash}`}
              type="button"
              data-holder={holder.lockArgs}
              data-current={current}
              title={holderTitle(holder, i > 0 ? holders[i - 1] : null)}
              onMouseEnter={() => setActive(i)}
              onMouseLeave={() => setActive(null)}
              onFocus={() => setActive(i)}
              onBlur={() => setActive(null)}
              onClick={() => setActive(i)}
              className={cn(
                'text-small text-text-bright flex min-w-[3px] items-center overflow-hidden whitespace-nowrap rounded-[2px] px-1.5 text-left',
                current && 'border-l-[3px] border-l-[#a0a8be]'
              )}
              style={{
                ...(current
                  ? { flexGrow: 1, flexShrink: 1, flexBasis: 0 }
                  : { flexGrow: 0, flexShrink: 0, flexBasis: `${share(holder)}%` }),
                backgroundColor: current ? CURRENT_FILL : PAST_FILL[i % 2],
              }}
            >
              <span className="truncate">{segmentLabel(holder, current, px)}</span>
            </button>
          );
        })}
      </div>
      <div
        data-testid="holder-axis"
        className="text-meta text-text-dim mt-1 flex justify-between tabular-nums"
      >
        <span>{day(start)}</span>
        <span>today</span>
      </div>
      <div
        role="status"
        aria-live="polite"
        aria-label="Code cell holder reading"
        className="border-base-border-subtle text-small text-text-dim mt-2.5 flex flex-wrap items-baseline gap-x-[18px] gap-y-1 border-t pt-2.5 tabular-nums"
      >
        <span>
          {isNow ? 'held now by' : 'held by'}{' '}
          <span className="text-text-bright">{holderName(reading.custody)}</span>
        </span>
        {isNow ? (
          <span>
            since <span className="text-text-bright">{day(reading.from.timestamp)}</span>
          </span>
        ) : (
          <span className="text-text-bright">{holderDates(reading)}</span>
        )}
        <span>{signerState(reading)}</span>
        <span>
          {formatCount(holders.length, 'holder')} · {formatCount(family.versionsCount, 'version')} ·{' '}
          {formatCount(holders.length - 1, 'handover')}
        </span>
        <span className="ml-auto">tap or hover</span>
      </div>
      <p data-role="caption" className="text-text text-body mt-3 max-w-[640px]">
        Whoever can spend the code cell can put new code in it; every {family.name} cell runs the
        new code from the next block.
      </p>
    </figure>
  );
}
