'use client';
// F2 group `objects` (round 2, OB-C1/C2/C3): the Collection board's Objects section — one allele
// gel of the whole collection (a lane per gene in DNA order, every variant a band; click a band to
// filter) and the newest objects; hovering a row draws its genotype across the gel. Built only
// from the API's pattern, counts and gallery rows. Consistency round (Main.dc.html): only the genes
// that vary get lanes (C9), the gel and the object table sit side by side, the table names the
// varying genes as columns with each object's composition, twenty a page (C11), and the filter is
// in the URL (C15).
import { useEffect, useMemo, useState, type CSSProperties } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useInRouterContext } from 'react-router-dom';
import { api, type CollectionInsights } from '@/lib/api';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { usePathname, useRouter, useSearchParams } from '@/src/navigation';
import { AssetReadState } from '@/components/asset/detail-shell';
import { AlleleGel, NotGenesNote } from '@/components/ui/allele-gel';
import { AppLink } from '@/components/ui/app-link';
import { PromptHeading } from '@/components/ui/prompt-heading';
import { Address } from '@/components/ui/address';
import { ClassMark } from '@/components/ui/class-mark';
import { CompositionBadge } from '@/components/ui/composition-mark';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { rowTagClass } from '@/components/ui/row-tag';
import {
  alleleLanes,
  droppedGenesCaption,
  geneLabel,
  geneOrder,
  geneUrlKey,
  genotypePoints,
  laneRule,
  traitDisplay,
  traitsOutsidePattern,
  type AlleleLane,
} from '@/lib/dob-genes';
import { GEL_INK } from '@/lib/design-tokens';
import { ALLELE_GEL } from '@/lib/allele-gel-layout';
import { formatBlockNumber } from '@/lib/format-block';
import type { CompositionTier } from '@/lib/asset-utils';
import { cn, formatUtc8Date, formatUtc8DateTime } from '@/lib/utils';
import { LockKind } from '@/components/ui/lock-kind';

// Consistency round C11 (Main.dc.html): twenty objects a page.
const NEWEST = 20;
const ROW_GRID =
  // visual pass: at 390 two columns, the cells flowing in pairs, every column kept
  'grid min-w-0 grid-cols-[minmax(0,1fr)_auto] items-center gap-x-3.5 gap-y-1';

const ONES = ['no', 'one', 'two', 'three', 'four', 'five', 'six', 'seven', 'eight', 'nine'];
/** "two genes vary" — the Objects heading's count of the genes drawn as lanes. */
function genesVary(n: number): string {
  const word = n < ONES.length ? ONES[n] : n.toLocaleString('en-US');
  return n === 1 ? `${word} gene varies` : `${word} genes vary`;
}

/** An object's number in its collection, else its short id (the row's key, as the board). */
function objectName(number: string | null, sporeId: string): string {
  if (number === null) return `${sporeId.slice(0, 10)}…${sporeId.slice(-4)}`;
  if (!/^(0|[1-9]\d*)$/.test(number))
    throw new Error(`${sporeId}: invalid object number ${number}`);
  // an object's number in its collection (not a block height)
  return '#' + BigInt(number).toLocaleString('en-US');
}

/** A gene value as a table cell: a Bitcoin pointer by its tail, anything else for people. */
function geneCellText(value: string): string {
  if (/^btcfs:\/\//i.test(value.trim())) return `…${value.trim().slice(-6)}`;
  return traitDisplay(value);
}

/** A gene value that is a number: it is never cut (no ellipsis inside a number). */
const NUMERIC = /^[\d,.]+$/;
/** Characters of a word value a gene cell keeps when its column is narrow (the rest "…"). */
const WORD_VALUE_CH = 8;
/** The swatch and its gap, in ch of the 12 px monospaced row text. */
const SWATCH_CH = 3;
/**
 * A gene cell's share of the objects as shown: " · 3" — how many carry the value. "of 1,298" is the
 * same in every cell, so it is the cell's and the header's title, and the columns keep their room.
 */
const geneShare = (count: number) => ` · ${count.toLocaleString('en-US')}`;
/**
 * Lane A follow-up: a gene column's minimum width in ch — the swatch, the shown part of the widest
 * value (a number whole, words up to 8 characters) and the widest share of its lane, so no cell
 * ever cuts its share or a number. The rows are monospaced, so characters measure the width.
 */
function geneColumnCh(lane: AlleleLane): number {
  return lane.bands.reduce((max, b) => {
    const text = geneCellText(b.value);
    const shown = NUMERIC.test(text) ? text.length : Math.min(text.length, WORD_VALUE_CH);
    return Math.max(max, SWATCH_CH + shown + geneShare(b.count).length);
  }, 0);
}

/** The gel's filters from the URL (C15): `?background=…&view=…`, as the D11 `?filter=`. */
function traitsFromUrl(params: URLSearchParams, lanes: readonly AlleleLane[] | null) {
  const traits: Record<string, string> = {};
  for (const lane of lanes ?? []) {
    const value = params.get(geneUrlKey(lane.gene));
    if (value !== null) traits[lane.gene] = value;
  }
  return traits;
}

export interface CollectionObjectsProps {
  id: string;
  title: string;
  insights: CollectionInsights;
  tier?: CompositionTier;
}

export function CollectionObjects(props: CollectionObjectsProps) {
  return <ScopedCollectionObjects key={props.id} {...props} />;
}

function ScopedCollectionObjects({ id, title, insights, tier }: CollectionObjectsProps) {
  const [row, setRow] = useState<string | null>(null);
  const [gene, setGene] = useState<string | null>(null);
  const pages = useCursorPagination();
  const genetics = insights.genetics;
  if (
    genetics.decodedItems + genetics.pendingItems + genetics.failedItems !==
      genetics.eligibleItems ||
    genetics.eligibleItems + genetics.nonDobItems !== genetics.liveItems
  )
    throw new Error(`${title}: live/eligible/decode coverage does not close`);
  if (genetics.complete && genetics.decodedItems !== genetics.eligibleItems)
    throw new Error(`${title}: complete frequencies require every eligible item decoded`);
  if (!genetics.complete && genetics.traits.length)
    throw new Error(`${title}: partial frequencies must not be presented as complete`);
  // C9: a raw-rule gene, a one-value gene and a gene unique to (almost) every object get no lane.
  const ruled = useMemo(
    () =>
      genetics.complete && genetics.traits.length
        ? laneRule(
            alleleLanes(genetics, geneOrder(insights.currentPattern), title),
            insights.currentPattern,
            genetics.eligibleItems
          )
        : null,
    [genetics, insights.currentPattern, title]
  );
  const lanes = ruled && ruled.lanes.length ? ruled.lanes : null;
  const outside = useMemo(
    () => (ruled ? traitsOutsidePattern(genetics, geneOrder(insights.currentPattern)) : []),
    [ruled, genetics, insights.currentPattern]
  );
  // C15: the gel's filter lives in the URL; outside a router it is this view's own state.
  const searchParams = useSearchParams();
  const pathname = usePathname();
  const router = useRouter();
  const inRouter = useInRouterContext();
  const urlTraits = useMemo(() => traitsFromUrl(searchParams, lanes), [searchParams, lanes]);
  const urlKey = JSON.stringify(urlTraits);
  const [traits, setTraits] = useState<Record<string, string>>(urlTraits);
  // The filter follows its URL, also when a link or a history entry changes it under this page.
  useEffect(() => setTraits(JSON.parse(urlKey) as Record<string, string>), [urlKey]);
  const ink = tier === 'btc_ckb' || tier === 'pure_ckb' ? tier : 'other';
  const filter = (name: string, value: string | null) => {
    pages.reset();
    const next = { ...traits };
    if (value === null || traits[name] === value) delete next[name];
    else next[name] = value;
    setTraits(next);
    if (inRouter) {
      const query = new URLSearchParams(searchParams);
      for (const lane of lanes ?? []) query.delete(geneUrlKey(lane.gene));
      for (const [g, v] of Object.entries(next)) query.set(geneUrlKey(g), v);
      // Replace: a filter is a view of this page, not a step of history.
      router.replace(query.size ? `${pathname}?${query}` : pathname, { scroll: false });
    }
  };
  const gallery = useQuery({
    queryKey: ['collection-gallery', id, traits, pages.cursor],
    queryFn: () => api.getCollectionGallery(id, { limit: NEWEST, cursor: pages.cursor, traits }),
  });
  const items = gallery.data?.data ?? [];
  const shown = gallery.data?.total;
  if (gallery.data && shown === undefined)
    throw new Error(`${title}: the gallery page states no total`);
  const overlays = lanes
    ? items.map((item) => ({
        key: item.sporeId,
        label: objectName(item.number, item.sporeId),
        points: genotypePoints(lanes, item.traits).points,
      }))
    : [];
  // The board's table names the genes that vary as its columns (two at most; the rest in titles).
  const columns = (lanes ?? []).slice(0, 2);
  const total = genetics.eligibleItems;
  // Each gene column is at least as wide as its widest cell (lane A follow-up); the object and
  // owner columns share the rest.
  const objectCols = [
    'minmax(0,1.3fr)',
    ...columns.map((lane, i) => `minmax(${geneColumnCh(lane)}ch,${i === 0 ? 1.1 : 0.8}fr)`),
    '88px',
    'minmax(0,1.6fr)',
    '88px',
  ].join(' ');
  // One line per object from md up (the share text needs the room); below it the cells pair up.
  const grid = cn(ROW_GRID, 'md:grid-cols-[var(--object-cols)]');
  const filtered = Object.keys(traits).length > 0;
  return (
    <section aria-label="Objects" className="min-w-0 space-y-[18px]">
      {/* Rubbing × Song print, phase 5 (A14): the objects, run on how many are live and how many
          genes vary among them (the count and the genes once rode inside the h2). */}
      <PromptHeading
        id="h-objects"
        command="Objects"
        args={`${genetics.liveItems.toLocaleString('en-US')} live${ruled ? ` · ${genesVary(ruled.lanes.length)}` : ''}`}
      />
      <div
        data-role="objects-columns"
        className={cn(
          'grid min-w-0 items-start gap-x-12 gap-y-8',
          // RESPONSIVE_LAYOUT rule 4: beside the table only when 440 px still draws it as a gel.
          (!lanes || lanes.length <= ALLELE_GEL.listAbove) && 'xl:grid-cols-[440px_minmax(0,1fr)]'
        )}
      >
        <div className="min-w-0 space-y-3">
          {lanes ? (
            <>
              <AlleleGel
                lanes={lanes}
                total={total}
                subject={title}
                ink={ink}
                overlays={overlays}
                activeOverlay={row}
                activeGene={gene}
                onGeneHover={setGene}
                onBandClick={(name, value) => filter(name, value)}
                selected={traits}
                labelOf={geneLabel}
                // C10: the readout row's own "tap or hover" is the section's one hint.
                idle="A band reads its variant, a row its genotype; a band filters the list."
              />
              <NotGenesNote traits={outside} />
            </>
          ) : ruled ? null : genetics.eligibleItems === 0 ? (
            <p className="text-text-dim text-small">
              This collection has no DOB objects to compare.
            </p>
          ) : (
            <p role="status" className="text-text-dim text-small">
              Allele frequencies and filtering await complete decoding:{' '}
              {genetics.decodedItems.toLocaleString('en-US')} of{' '}
              {genetics.eligibleItems.toLocaleString('en-US')} decoded.
            </p>
          )}
          {ruled && ruled.dropped.length > 0 && (
            <p data-role="caption" className="text-text-dim text-small max-w-[400px]">
              {droppedGenesCaption(ruled.dropped, total)}
            </p>
          )}
        </div>
        <div className="min-w-0 space-y-1.5">
          <div className="text-text-dim text-small flex flex-wrap items-baseline justify-between gap-x-4 gap-y-2">
            <span>
              Newest first ·{' '}
              <span className="text-text tabular-nums">
                {shown !== undefined && filtered
                  ? `${shown.toLocaleString('en-US')} of ${total.toLocaleString('en-US')} objects`
                  : `${(shown ?? total).toLocaleString('en-US')} objects`}
              </span>
            </span>
            {filtered && (
              <span className="flex flex-wrap gap-2">
                {Object.entries(traits).map(([name, value]) => (
                  <button
                    key={name}
                    type="button"
                    aria-label={`Clear ${name} = ${value}`}
                    onClick={() => filter(name, null)}
                    className="border-jade bg-base-surface text-text-bright text-meta h-[22px] rounded border px-2"
                  >
                    {geneLabel(name)} {traitDisplay(value)} ×
                  </button>
                ))}
              </span>
            )}
          </div>
          {gallery.data ? (
            <>
              <AssetReadState label="newest objects" error={gallery.error} retained />
              <div
                role="table"
                aria-label="Newest objects"
                className="text-small min-w-0"
                style={{ '--object-cols': objectCols } as CSSProperties}
              >
                <div role="row" className={cn(grid, 'text-text-dim text-meta hidden h-7 md:grid')}>
                  <span role="columnheader">Object</span>
                  {columns.map((lane) => (
                    <span
                      key={lane.gene}
                      role="columnheader"
                      title={`${lane.gene} · beside each value, how many of the ${total.toLocaleString('en-US')} objects carry it`}
                    >
                      {geneLabel(lane.gene)}
                    </span>
                  ))}
                  <span role="columnheader">Made of</span>
                  <span role="columnheader">Owner</span>
                  <span role="columnheader" className="text-right">
                    Minted
                  </span>
                </div>
                {items.map((item) => {
                  const name = objectName(item.number, item.sporeId);
                  const lit = row === item.sporeId;
                  const itemTier = item.mediaProfile?.tier ?? null;
                  // Table-rows L2-4 / C11: one line per object; what does not fit it (its id, the
                  // genes past the columns, its DNA, the full owner) is the row's title.
                  const owner = item.ownerLockHash
                    ? `owner ${item.ownerAddress ?? item.ownerLockHash} · ${item.lockKind ?? item.lockCodeHash}`
                    : 'no live owner';
                  const genes = (lanes ?? []).flatMap((lane) => {
                    const own = item.traits.find((t) => t.name === lane.gene);
                    return own ? [`${geneLabel(lane.gene)} ${traitDisplay(own.value)}`] : [];
                  });
                  return (
                    <div
                      key={item.sporeId}
                      role="row"
                      title={[
                        `${title} ${name}`,
                        item.sporeId,
                        ...genes,
                        owner,
                        `minted ${formatUtc8DateTime(item.chainTime)} in block ${formatBlockNumber(item.createdAtBlock)}`,
                        ...(item.dnaHex === null ? [] : [`DNA ${item.dnaHex}`]),
                      ].join(' · ')}
                      data-active={lit ? 'true' : undefined}
                      onMouseEnter={() => setRow(item.sporeId)}
                      onMouseLeave={() => setRow(null)}
                      onFocus={() => setRow(item.sporeId)}
                      onBlur={() => setRow(null)}
                      className={cn(
                        grid,
                        'border-base-border-subtle min-h-[41px] border-t py-1 transition-colors',
                        lit && 'bg-base-surface'
                      )}
                    >
                      <span role="cell" className="flex min-w-0 items-center gap-2">
                        <ClassMark kind="objects" size={12} />
                        <AppLink
                          href={`/objects/${item.sporeId}`}
                          className="text-body text-text-bright min-w-0 truncate leading-[18px]"
                        >
                          {name}
                        </AppLink>
                      </span>
                      {columns.map((lane) => {
                        const own = item.traits.find((t) => t.name === lane.gene);
                        const band = own ? lane.bands.find((b) => b.value === own.value) : null;
                        return (
                          <span
                            key={lane.gene}
                            role="cell"
                            data-gene-cell
                            className="flex min-w-0 items-center whitespace-nowrap"
                            title={
                              own
                                ? `${geneLabel(lane.gene)} = ${own.value}${band ? ` · ${band.count.toLocaleString('en-US')} of ${total.toLocaleString('en-US')} objects` : ''}`
                                : undefined
                            }
                          >
                            {own ? (
                              <>
                                <i
                                  aria-hidden="true"
                                  className="mr-1.5 inline-block h-2.5 w-2.5 shrink-0 rounded-sm"
                                  style={{
                                    background: band?.colour ?? GEL_INK[ink][1],
                                    opacity: band?.colour ? 1 : 0.8,
                                  }}
                                />
                                {/* Words may be cut; a number and the share never are. */}
                                <span
                                  data-role="gene-value"
                                  className={cn(
                                    'text-text',
                                    NUMERIC.test(geneCellText(own.value))
                                      ? 'shrink-0'
                                      : 'min-w-0 truncate'
                                  )}
                                >
                                  {geneCellText(own.value)}
                                </span>
                                {band && (
                                  <span
                                    data-role="gene-share"
                                    className="text-text-ghost shrink-0 whitespace-nowrap"
                                  >
                                    {geneShare(band.count)}
                                  </span>
                                )}
                              </>
                            ) : (
                              <span className="text-text-dim">—</span>
                            )}
                          </span>
                        );
                      })}
                      <span role="cell" className="min-w-0">
                        {itemTier ? (
                          <CompositionBadge tier={itemTier} size="sm" />
                        ) : (
                          <span className="text-text-dim">not recorded</span>
                        )}
                      </span>
                      <span role="cell" className="min-w-0 truncate">
                        {item.ownerLockHash ? (
                          <>
                            <Address
                              address={item.ownerAddress ?? item.ownerLockHash}
                              startChars={8}
                              endChars={6}
                              className="text-small"
                            />{' '}
                            <LockKind
                              lockKind={item.lockKind}
                              lockCodeHash={item.lockCodeHash}
                              className={rowTagClass()}
                            />
                          </>
                        ) : (
                          <span className="text-text-dim">No live owner</span>
                        )}
                      </span>
                      <span role="cell" className="text-text-dim tabular-nums md:text-right">
                        {/* R-2: the minting block's chain time, on the site's UTC+8 clock. */}
                        <AppLink
                          href={`/blocks/${item.createdAtBlock}`}
                          title={`${formatBlockNumber(item.createdAtBlock)} · ${formatUtc8DateTime(item.chainTime)}`}
                        >
                          {formatUtc8Date(item.chainTime)}
                        </AppLink>
                      </span>
                    </div>
                  );
                })}
              </div>
              {items.length === 0 && <p className="text-text-dim">No matching objects.</p>}
              <CursorPagination
                total={gallery.data.total}
                totalLabel="objects"
                pageSize={NEWEST}
                page={pages.page}
                currentCount={items.length}
                hasMore={gallery.data.hasMore}
                hasPrevious={pages.hasPrevious}
                onNext={() => pages.goToNext(gallery.data!.nextCursor)}
                onPrevious={pages.goToPrevious}
              />
            </>
          ) : (
            <AssetReadState
              label="newest objects"
              error={gallery.error}
              loading={gallery.isLoading}
            />
          )}
        </div>
      </div>
    </section>
  );
}

/** Tier 2 (OB-C11): how much of the collection has been decoded, with exact counts. */
export function DecodeCoverage({ genetics }: { genetics: CollectionInsights['genetics'] }) {
  const n = (v: number) => v.toLocaleString('en-US');
  return (
    <p className="text-text-dim text-meta">
      {n(genetics.decodedItems)} of {n(genetics.eligibleItems)} DOB objects decoded ·{' '}
      {n(genetics.pendingItems)} pending · {n(genetics.failedItems)} failed ·{' '}
      {n(genetics.nonDobItems)} non-DOB objects · {n(genetics.liveItems)} live
    </p>
  );
}
