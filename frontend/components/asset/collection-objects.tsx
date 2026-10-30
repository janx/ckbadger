'use client';
// F2 group `objects` (round 2, OB-C1/C2/C3): the Collection board's Objects section — one allele
// gel of the whole collection (a lane per gene in DNA order, every variant a band; click a band to
// filter) and a "Newest" list whose rows carry the object's DNA as a mini gel; hovering a row
// draws its genotype across the gel. Built only from the API's pattern, counts and gallery rows.
import { useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, type CollectionInsights } from '@/lib/api';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { AssetReadState } from '@/components/asset/detail-shell';
import { AlleleGel } from '@/components/ui/allele-gel';
import { MiniGel } from '@/components/ui/sequencing-gel';
import { AppLink } from '@/components/ui/app-link';
import { Address } from '@/components/ui/address';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { alleleLanes, geneOrder, genotypePoints, traitDisplay } from '@/lib/dob-genes';
import { GEL_INK } from '@/lib/design-tokens';
import { formatBlockNumber } from '@/lib/format-block';
import type { CompositionTier } from '@/lib/asset-utils';
import { cn } from '@/lib/utils';

const NEWEST = 8;
const ROW_GRID =
  'grid min-w-0 items-center gap-x-3.5 gap-y-1 sm:grid-cols-[minmax(0,150px)_minmax(0,120px)_minmax(0,1fr)_132px_100px]';

function objectNumber(number: string | null, sporeId: string): string {
  if (number === null) return `${sporeId.slice(0, 10)}…${sporeId.slice(-4)}`;
  if (!/^(0|[1-9]\d*)$/.test(number))
    throw new Error(`${sporeId}: invalid object number ${number}`);
  return `#${BigInt(number).toLocaleString('en-US')}`;
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
  const [traits, setTraits] = useState<Record<string, string>>({});
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
  const gallery = useQuery({
    queryKey: ['collection-gallery', id, traits, pages.cursor],
    queryFn: () => api.getCollectionGallery(id, { limit: NEWEST, cursor: pages.cursor, traits }),
  });
  const lanes = useMemo(
    () =>
      genetics.complete && genetics.traits.length
        ? alleleLanes(genetics, geneOrder(insights.currentPattern), title)
        : null,
    [genetics, insights.currentPattern, title]
  );
  const ink = tier === 'btc_ckb' || tier === 'pure_ckb' ? tier : 'other';
  const filter = (name: string, value: string | null) => {
    pages.reset();
    setTraits((prev) => {
      const next = { ...prev };
      if (value === null || prev[name] === value) delete next[name];
      else next[name] = value;
      return next;
    });
  };
  const items = gallery.data?.data ?? [];
  const overlays = lanes
    ? items.map((item) => ({
        key: item.sporeId,
        label: objectNumber(item.number, item.sporeId),
        points: genotypePoints(lanes, item.traits).points,
      }))
    : [];
  return (
    <section aria-labelledby="h-objects" className="min-w-0 space-y-3.5">
      <div className="flex flex-wrap items-baseline justify-between gap-4">
        <h2 id="h-objects" className="text-text-bright text-[15px]">
          Objects
        </h2>
        {lanes && (
          <span className="text-text-dim text-[12px]">
            {genetics.eligibleItems.toLocaleString('en-US')} · {lanes.length} gene
            {lanes.length === 1 ? '' : 's'} · how common each variant is
          </span>
        )}
      </div>
      {lanes ? (
        <AlleleGel
          lanes={lanes}
          total={genetics.eligibleItems}
          subject={title}
          ink={ink}
          overlays={overlays}
          activeOverlay={row}
          activeGene={gene}
          onGeneHover={setGene}
          onBandClick={(name, value) => filter(name, value)}
          selected={traits}
        />
      ) : genetics.eligibleItems === 0 ? (
        <p className="text-text-dim text-[12px]">This collection has no DOB objects to compare.</p>
      ) : (
        <p role="status" className="text-text-dim text-[12px]">
          Allele frequencies and filtering await complete decoding:{' '}
          {genetics.decodedItems.toLocaleString('en-US')} of{' '}
          {genetics.eligibleItems.toLocaleString('en-US')} decoded.
        </p>
      )}
      <div className="flex flex-wrap items-baseline justify-between gap-4 pt-2.5">
        <span className="text-text text-[13px]">Newest</span>
        <span className="text-text-dim flex flex-wrap items-baseline gap-3 text-[12px]">
          {lanes && <span>hover a row to see its genotype · click a band to filter</span>}
          {Object.entries(traits).map(([name, value]) => (
            <button
              key={name}
              type="button"
              aria-label={`Clear ${name} = ${value}`}
              onClick={() => filter(name, null)}
              className="border-base-border text-text-bright rounded border px-2"
            >
              {name} = {traitDisplay(value)} ×
            </button>
          ))}
        </span>
      </div>
      {gallery.data ? (
        <>
          <AssetReadState label="newest objects" error={gallery.error} retained />
          <div role="table" aria-label="Newest objects" className="min-w-0 text-[12px]">
            <div
              role="row"
              className={cn(
                ROW_GRID,
                'text-text-dim border-base-border hidden border-b pb-1.5 text-[11px] sm:grid'
              )}
            >
              <span role="columnheader">Object</span>
              <span role="columnheader">Id</span>
              <span role="columnheader">Owner</span>
              <span role="columnheader">DNA · read →</span>
              <span role="columnheader" className="text-right">
                Minted
              </span>
            </div>
            {items.map((item) => {
              const name = objectNumber(item.number, item.sporeId);
              const lit = row === item.sporeId;
              return (
                <div
                  key={item.sporeId}
                  role="row"
                  data-active={lit ? 'true' : undefined}
                  onMouseEnter={() => setRow(item.sporeId)}
                  onMouseLeave={() => setRow(null)}
                  onFocus={() => setRow(item.sporeId)}
                  onBlur={() => setRow(null)}
                  className={cn(
                    ROW_GRID,
                    'border-base-elevated min-h-11 border-b py-1.5 transition-colors',
                    lit && 'bg-base-surface'
                  )}
                >
                  <span role="cell" className="min-w-0">
                    <AppLink href={`/objects/${item.sporeId}`} className="text-text-bright">
                      {title} {name}
                    </AppLink>
                  </span>
                  <span role="cell" className="text-text-dim min-w-0 truncate">
                    {item.sporeId.slice(0, 10)}…{item.sporeId.slice(-4)}
                  </span>
                  <span role="cell" className="min-w-0 truncate">
                    {item.ownerLockHash ? (
                      <Address
                        address={item.ownerAddress ?? item.ownerLockHash}
                        startChars={12}
                        endChars={6}
                        className="text-[12px]"
                      />
                    ) : (
                      <span className="text-text-dim">No live owner</span>
                    )}
                  </span>
                  <span role="cell">
                    {item.dnaHex !== null ? (
                      <MiniGel
                        dna={item.dnaHex}
                        label={`${item.sporeId} DNA`}
                        ink={GEL_INK[ink][1]}
                      />
                    ) : (
                      <span className="text-text-dim">No DNA</span>
                    )}
                  </span>
                  <span role="cell" className="text-text-dim sm:text-right">
                    <AppLink href={`/blocks/${item.createdAtBlock}`}>
                      {formatBlockNumber(item.createdAtBlock)}
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
            hasMore={gallery.data.hasMore}
            hasPrevious={pages.hasPrevious}
            onNext={() => pages.goToNext(gallery.data!.nextCursor)}
            onPrevious={pages.goToPrevious}
          />
        </>
      ) : (
        <AssetReadState label="newest objects" error={gallery.error} loading={gallery.isLoading} />
      )}
    </section>
  );
}

/** Tier 2 (OB-C11): how much of the collection has been decoded, with exact counts. */
export function DecodeCoverage({ genetics }: { genetics: CollectionInsights['genetics'] }) {
  const n = (v: number) => v.toLocaleString('en-US');
  return (
    <p className="text-text-dim text-[11px]">
      {n(genetics.decodedItems)} of {n(genetics.eligibleItems)} DOB objects decoded ·{' '}
      {n(genetics.pendingItems)} pending · {n(genetics.failedItems)} failed ·{' '}
      {n(genetics.nonDobItems)} non-DOB objects · {n(genetics.liveItems)} live
    </p>
  );
}
