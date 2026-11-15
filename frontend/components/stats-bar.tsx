'use client';

import Link from '@/components/ui/link';
import { usePathname } from '@/src/navigation';
import { useNetworkStats } from '@/hooks/useNetworkStats';
import { usePhosphor } from '@/hooks/usePhosphor';
import { useRealtimeStore } from '@/hooks/useRealtimeStore';
import type { NetworkStats } from '@/lib/api';
import { BAR_EMPTY, BAR_FULL, blockBarCells } from '@/lib/blocks-text';
import { isGenesisEpoch, parseEpoch, type EpochInfo } from '@/lib/epoch';
import { formatBlockNumber } from '@/lib/format-block';
import { cn } from '@/lib/utils';

/**
 * The chain vitals strip under the header, on every page: block, epoch progress, block time, hash
 * rate and difficulty, then — on a page that holds the realtime socket — "● Live" at its right end
 * (Main.dc.html, TX-01). It replaces the homepage's old hero numbers, epoch card and floating LIVE
 * pill.
 *
 * From md up it is one wrapping row on every page. Below md (UI review 2026-11-14, PA-14; the
 * rule in docs/RESPONSIVE_LAYOUT.md "Vitals strip") the home page has the whole strip in three
 * lines grouped by meaning, never a row to pan: the block with Live at the right end, the epoch
 * with its block-character bar, then the three mining figures — inline pairs packed to the left
 * from 640 px, three equal columns with the label over the value below it. One grid places the
 * items in document order, so from md up the same items flow as the row. Every other page has one
 * line below md, the block, the epoch's number and bar, and the live mark at the end (its dot
 * alone below 640): the mining figures and the epoch's position and time left are the home page's
 * there. The three lines took 212 of an 844 px first screen with the header and the status line.
 */
export function VitalsStrip() {
  const { data: stats, error } = useNetworkStats();
  const isConnected = useRealtimeStore((s) => s.isConnected);
  const held = useRealtimeStore((s) => s.socketHeld);
  const form: PhoneForm = usePathname() === '/' ? 'lines' : 'line';

  return (
    <section
      aria-label="Chain vitals"
      data-testid="vitals-strip"
      data-phone-form={form}
      className={cn(
        'min-w-0 flex-1 items-baseline gap-y-2 py-2.5 font-mono md:flex md:flex-wrap md:gap-x-6 md:gap-y-2.5 md:py-3',
        form === 'lines'
          ? 'grid grid-cols-3 gap-x-4 sm:grid-cols-[max-content_max-content_max-content_1fr]'
          : 'flex flex-wrap gap-x-3'
      )}
    >
      {error ? (
        <VitalsError message={error.message} form={form} />
      ) : stats ? (
        <Vitals stats={stats} form={form} />
      ) : (
        <VitalsLoading form={form} />
      )}
      {held && <LiveMark connected={isConnected} form={form} />}
    </section>
  );
}

/**
 * The strip below md: `lines`, the home page's three lines, or `line`, every other page's one.
 * From md up both are the same row.
 */
type PhoneForm = 'lines' | 'line';

/**
 * Where each item sits below md (ignored from md up, where the strip is a flex row). In the three
 * lines the block spans the tracks left of Live on line 1, the epoch and any strip-wide message
 * take a whole line, and each mining figure takes one track of line 3. The one line places
 * nothing: its items follow each other, Live pushed to the end, and what is the home page's is
 * not drawn below md.
 */
const PLACE: Record<PhoneForm, { block: string; line: string; live: string; homeOnly: string }> = {
  lines: {
    block: 'col-span-2 sm:col-span-3',
    line: 'col-span-3 sm:col-span-4',
    live: 'row-start-1 col-start-3 sm:col-start-4',
    homeOnly: '',
  },
  line: { block: '', line: '', live: '', homeOnly: 'max-md:hidden' },
};

/** A mining figure: label over value below 640 px, label beside value from 640 px up. */
const STACKED = 'flex flex-col gap-px sm:flex-row sm:items-baseline sm:gap-1.5 md:gap-2';
const INLINE = 'flex items-baseline gap-1.5 md:gap-2';

const LABEL = 'text-text-dim text-meta md:text-small';
const VALUE =
  'text-text-bright group-hover:text-jade text-body font-medium tabular-nums transition-colors';

/** A vital: its label and its value, which flares when it changes (material pass v2, move 4). */
function Vital({
  label,
  href,
  layout,
  value,
  form,
}: {
  label: string;
  href?: string;
  /** `block` is line 1's inline pair; a mining figure stacks below 640 px. */
  layout: 'block' | 'mining';
  value: string;
  form: PhoneForm;
}) {
  const flare = usePhosphor(value);
  const body = (
    <>
      <span className={LABEL}>{label}</span>
      <span className={cn(VALUE, flare)}>{value}</span>
    </>
  );
  const cls = cn(
    'group',
    layout === 'block' ? [INLINE, PLACE[form].block] : [STACKED, PLACE[form].homeOnly]
  );
  return href ? (
    <Link href={href} className={cls}>
      {body}
    </Link>
  ) : (
    <div className={cls}>{body}</div>
  );
}

const DOT_BOX = 'h-[7px] w-[7px] shrink-0 self-center rounded-full';

/**
 * The API's average block time is a seconds string ("8.43s"); the board writes the unit apart
 * ("8.43 s", TX-05). Anything else is refused, not reshaped.
 */
function BlockTime({ value, form }: { value: string | null; form: PhoneForm }) {
  if (value === null) return <Vital label="Block time" layout="mining" value="—" form={form} />;
  const seconds = /^(\d+(?:\.\d+)?)s$/.exec(value);
  if (!seconds)
    return (
      <div
        role="alert"
        data-testid="vitals-block-time-error"
        className={cn(STACKED, PLACE[form].homeOnly)}
      >
        <span className={LABEL}>Block time</span>
        <span className="text-rouge text-small">{`block time ${JSON.stringify(value)} is not <seconds>s`}</span>
      </div>
    );
  return <Vital label="Block time" layout="mining" value={`${seconds[1]} s`} form={form} />;
}

function Vitals({ stats, form }: { stats: NetworkStats; form: PhoneForm }) {
  return (
    <>
      <Vital
        label="Block"
        href={`/blocks/${stats.latestBlock}`}
        layout="block"
        value={formatBlockNumber(stats.latestBlock)}
        form={form}
      />
      <EpochVital
        raw={stats.epoch}
        block={stats.latestBlock}
        estimatedLeft={stats.estimatedEpochTime}
        form={form}
      />
      <BlockTime value={stats.avgBlockTime} form={form} />
      <Vital
        label="Hash rate"
        href="/charts/hash-rate"
        layout="mining"
        value={stats.hashRate ?? '—'}
        form={form}
      />
      <Vital
        label="Difficulty"
        href="/charts/difficulty"
        layout="mining"
        value={stats.difficulty}
        form={form}
      />
    </>
  );
}

/** The tip header's epoch, block `block`'s (`latestBlock`, read with it), or why it can't be. */
function readEpoch(raw: string, block: number): { epoch: EpochInfo } | { error: string } {
  try {
    return { epoch: parseEpoch(raw, block) };
  } catch (e) {
    return { error: e instanceof Error ? e.message : String(e) };
  }
}

/**
 * Epoch number, its progress in block characters, `index / length` and the time left — or why it
 * can't. Its text never wraps: the parts never stack into a column. With the tip at genesis (a
 * fresh chain that has mined nothing) the header states the zero epoch, `0 / 0`: no length, so no
 * bar, and in the bar's place the reason.
 */
function EpochVital({
  raw,
  block,
  estimatedLeft,
  form,
}: {
  raw: string;
  block: number;
  estimatedLeft: string | null;
  form: PhoneForm;
}) {
  const read = readEpoch(raw, block);
  const flare = usePhosphor('error' in read ? null : read.epoch.number);
  if ('error' in read) {
    return (
      <div
        role="alert"
        data-testid="vitals-epoch-error"
        className={cn('flex items-baseline gap-2', PLACE[form].line)}
      >
        <span className={LABEL}>Epoch</span>
        <span className="text-rouge text-small">{`unreadable: ${read.error}`}</span>
      </div>
    );
  }
  const { epoch } = read;
  const genesis = isGenesisEpoch(epoch);
  const index = epoch.index.toLocaleString('en-US');
  const length = epoch.length.toLocaleString('en-US');
  const position = genesis
    ? 'the genesis block: its header states no epoch length'
    : `block ${index} of ${length}`;
  return (
    <Link
      href="/charts/epoch-time-length"
      data-testid="vitals-epoch"
      aria-label={`Epoch ${epoch.number.toLocaleString('en-US')}, ${position}, ${estimatedLeft === null ? 'remaining time unavailable' : `about ${estimatedLeft} left`}`}
      className={cn(
        'group flex items-center gap-x-1.5 whitespace-nowrap md:gap-x-2',
        PLACE[form].line
      )}
    >
      <span className={LABEL}>Epoch</span>
      <span className={cn(VALUE, flare)}>{epoch.number.toLocaleString('en-US')}</span>
      {genesis ? (
        <span data-testid="vitals-epoch-genesis" className={LABEL}>
          genesis: no length
        </span>
      ) : (
        <EpochBar fraction={epoch.index / epoch.length} />
      )}
      {/* the epoch's position and time left: the home page's below md (the link's label says
          them on every page) */}
      <span className={cn('text-text text-meta md:text-small tabular-nums', PLACE[form].homeOnly)}>
        {index} / {length}
      </span>
      <span className={cn(LABEL, PLACE[form].homeOnly)}>
        {estimatedLeft === null ? '—' : `≈ ${estimatedLeft} left`}
      </span>
    </Link>
  );
}

/** The epoch bar's cells: ten from 640 px; five on a phone, whose line also holds the number,
 * `index / length` and the time left at 360 px (a ten-cell bar there pushes the line past the
 * width once the index has four digits). */
const EPOCH_CELLS = { narrow: 5, wide: 10 } as const;

/**
 * The epoch's progress as block characters (rubbing × Song print, phase 2): the filled cells in
 * celadon, the rest `░` in the dim ink (lib/blocks-text). Drawn for the eye only: the link's
 * label says the numbers.
 */
function EpochBar({ fraction }: { fraction: number }) {
  return (
    <span
      aria-hidden="true"
      data-testid="vitals-epoch-bar"
      className="text-meta md:text-small shrink-0 leading-none"
    >
      {(['narrow', 'wide'] as const).map((form) => {
        const width = EPOCH_CELLS[form];
        const full = blockBarCells(fraction, width);
        return (
          <span
            key={form}
            data-cells={width}
            className={form === 'narrow' ? 'sm:hidden' : 'hidden sm:inline'}
          >
            <span data-part="full" className="text-jade">
              {BAR_FULL.repeat(full)}
            </span>
            <span data-part="empty" className="text-text-dim">
              {BAR_EMPTY.repeat(width - full)}
            </span>
          </span>
        );
      })}
    </span>
  );
}

/**
 * A failed /statistics/network, said plainly: no stale or placeholder numbers. Like the homepage
 * figures, an error wins over data until a fetch succeeds again.
 */
function VitalsError({ message, form }: { message: string; form: PhoneForm }) {
  return (
    <span
      role="alert"
      data-testid="vitals-error"
      className={cn('text-rouge text-small', PLACE[form].line)}
    >
      {`Chain vitals unavailable: ${message}`}
    </span>
  );
}

/** The labels in the places their figures will take. */
const LOADING_SLOTS = [
  { label: 'Block', cls: (form: PhoneForm) => cn(INLINE, PLACE[form].block) },
  { label: 'Epoch', cls: (form: PhoneForm) => cn(INLINE, PLACE[form].line) },
  { label: 'Block time', cls: (form: PhoneForm) => cn(STACKED, PLACE[form].homeOnly) },
  { label: 'Hash rate', cls: (form: PhoneForm) => cn(STACKED, PLACE[form].homeOnly) },
  { label: 'Difficulty', cls: (form: PhoneForm) => cn(STACKED, PLACE[form].homeOnly) },
] as const;

function LoadingVital({ label, cls }: { label: string; cls: string }) {
  return (
    <div className={cls}>
      <span className={LABEL}>{label}</span>
      <span aria-hidden="true" className="bg-base-elevated inline-block h-3 w-16 animate-pulse" />
    </div>
  );
}

function VitalsLoading({ form }: { form: PhoneForm }) {
  return (
    <>
      {LOADING_SLOTS.map((slot) => (
        <LoadingVital key={slot.label} label={slot.label} cls={slot.cls(form)} />
      ))}
    </>
  );
}

/**
 * Whether the page's realtime socket is up (Main.dc.html "● Live", TX-01): a jade dot and "Live"
 * when it is, a dim dot and "Not live" while it is not yet or no longer connected. Only a page
 * that holds the socket shows it. On the one line below 640 (PA-14) it is its dot: at 360 the line
 * holds the block (124.8 px), the epoch and its bar (124.8 px) and the dot with two 12 px gaps,
 * 280.6 of 328 px, where "Not live" (72.6 px with its dot) would make it 346.2. The word stays for a
 * screen reader and in the title, and the status line's mode block says it on every page.
 */
function LiveMark({ connected, form }: { connected: boolean; form: PhoneForm }) {
  return (
    <span
      role="status"
      data-testid="vitals-live"
      data-live={connected}
      title={
        connected ? 'Live: realtime updates connected' : 'Not live: realtime updates not connected'
      }
      className={cn(
        'text-small ml-auto flex items-center gap-2',
        PLACE[form].live,
        connected ? 'text-jade' : 'text-text-dim'
      )}
    >
      <span
        aria-hidden="true"
        className={`${DOT_BOX} ${connected ? 'bg-jade' : 'bg-base-border'}`}
      />
      <span className={form === 'line' ? 'max-sm:sr-only' : undefined}>
        {connected ? 'Live' : 'Not live'}
      </span>
    </span>
  );
}
