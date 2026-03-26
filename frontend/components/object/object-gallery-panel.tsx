'use client';

import { useMemo } from 'react';
import Link from '@/components/ui/link';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
  TerminalPanelFooter,
} from '@/components/ui/terminal-panel';
import { Badge } from '@/components/ui/page-header';
import { HexDisplay } from '@/components/ui/hex-display';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { formatNumber } from '@/lib/utils';

export const GALLERY_PAGE_SIZE = 18;
import type {
  CollectionItem,
  SporeNft,
  ItemStatusFilter,
  CursorPaginatedResponse,
} from '@/lib/api';

/* ---------- Cell Glyph ---------- */

/**
 * CKB-native hash visualization: 3 concentric arcs representing
 * a CKB cell's layered structure.
 *
 *   Outer arc  = Lock script  (ownership / protection)
 *   Middle arc = Type script  (rules / structure)
 *   Inner arc  = Data         (stored content)
 *   Center dot = Cell nucleus (identity core)
 *
 * Each arc's hue, sweep angle, and rotation are derived from the hash,
 * making every cell visually unique — like viewing a living cell
 * cross-section under a microscope.
 */
function CellGlyph({ hash, size = 36 }: { hash: string; size?: number }) {
  const glyph = useMemo(() => {
    const clean = hash.replace('0x', '').toLowerCase();
    const byte = (i: number) => parseInt(clean.slice(i * 2, i * 2 + 2) || '0', 16);

    // 3 hues from different parts of the hash, spread apart for contrast
    const hue1 = ((byte(0) << 8) | byte(1)) % 360;
    const hue2 = (hue1 + 50 + (byte(2) % 180)) % 360;
    const hue3 = (hue1 + 140 + (byte(3) % 80)) % 360;

    // Arc sweep: 35%-90% of circumference (enough variation, never a full ring)
    const sweep = (b: number) => 0.35 + (b / 255) * 0.55;
    // Rotation: 0-360°
    const rot = (b: number) => (b / 255) * 360;

    return {
      arcs: [
        { r: 14, hue: hue1, sweep: sweep(byte(4)), rotation: rot(byte(5)), width: 2.5 },
        { r: 10, hue: hue2, sweep: sweep(byte(6)), rotation: rot(byte(7)), width: 2.5 },
        { r: 6, hue: hue3, sweep: sweep(byte(8)), rotation: rot(byte(9)), width: 2 },
      ],
      dotHue: hue1,
      dotR: 1.8 + (byte(10) % 3) * 0.4,
    };
  }, [hash]);

  const cx = size / 2;
  const cy = size / 2;
  const s = size / 36; // scale for non-default sizes

  return (
    <svg
      width={size}
      height={size}
      viewBox={`0 0 ${size} ${size}`}
      className="shrink-0"
      aria-hidden
    >
      {/* Dark background */}
      <circle cx={cx} cy={cy} r={cx - 0.5} fill="hsl(0, 0%, 7%)" />

      {/* Concentric arcs: lock → type → data */}
      {glyph.arcs.map((arc, i) => {
        const r = arc.r * s;
        const circ = 2 * Math.PI * r;
        const dash = circ * arc.sweep;
        const gap = circ - dash;
        return (
          <circle
            key={i}
            cx={cx}
            cy={cy}
            r={r}
            fill="none"
            stroke={`hsl(${arc.hue}, 55%, 50%)`}
            strokeWidth={arc.width * s}
            strokeLinecap="round"
            strokeDasharray={`${dash} ${gap}`}
            strokeDashoffset={-circ * (arc.rotation / 360)}
            opacity={0.85}
          />
        );
      })}

      {/* Cell nucleus */}
      <circle cx={cx} cy={cy} r={glyph.dotR * s} fill={`hsl(${glyph.dotHue}, 60%, 58%)`} />
    </svg>
  );
}

/* ---------- Token index extraction ---------- */

/**
 * For sequential-ID standards (mNFT), extract a human-readable token number
 * from the trailing bytes of the ID. Returns null for random hashes.
 */
function extractTokenIndex(nftId: string, standard: string): number | null {
  if (standard.toLowerCase() !== 'm-nft') return null;
  const clean = nftId.replace('0x', '');
  if (clean.length < 8) return null;
  const tail = clean.slice(-8);
  const num = parseInt(tail, 16);
  // Only treat as token index if it's a reasonable number
  if (num <= 999999) return num;
  return null;
}

/* ---------- Content type helpers (Spore) ---------- */

function getContentTypeIcon(contentType: string | null | undefined): string {
  const normalized = (contentType ?? '').toLowerCase();
  if (!normalized) return '\u{1F4E6}'; // 📦
  if (normalized.startsWith('image/') || normalized.startsWith('ipfs/image'))
    return '\u{1F5BC}\uFE0F'; // 🖼️
  if (normalized.startsWith('video/') || normalized.startsWith('ipfs/video')) return '\u{1F3AC}'; // 🎬
  if (normalized.startsWith('audio/') || normalized.startsWith('ipfs/audio')) return '\u{1F3B5}'; // 🎵
  if (normalized.startsWith('text/')) return '\u{1F4C4}'; // 📄
  return '\u{1F4E6}'; // 📦
}

/* ---------- Object Card (CollectionItem) ---------- */

function objectDetailHref(item: CollectionItem): string | null {
  const std = item.standard.toLowerCase();
  if (std === 'm-nft') return `/objects/mnft/${item.nftId}`;
  if (std === 'did_ckb' || std === 'did:ckb')
    return `/identities/did/${encodeURIComponent(item.nftId)}`;
  if (std === 'dotbit') return `/identities/dotbit/${encodeURIComponent(item.nftId)}`;
  return null;
}

function inactiveStatusLabel(item: CollectionItem): string {
  const std = item.standard.toLowerCase();
  if (std === 'did_ckb' || std === 'did:ckb' || std === 'dotbit') return 'Recycled';
  return 'Burned';
}

function ObjectCard({ item }: { item: CollectionItem }) {
  const href = objectDetailHref(item);
  const tokenIndex = extractTokenIndex(item.nftId, item.standard);
  const hasName = !!item.name;

  // Primary label: name > token number > truncated hash
  const primaryLabel = hasName ? item.name! : tokenIndex !== null ? `#${tokenIndex}` : null;

  const CardLink = ({ children, className }: { children: React.ReactNode; className?: string }) =>
    href ? (
      <Link href={href} className={className}>
        {children}
      </Link>
    ) : (
      <span className={className}>{children}</span>
    );

  return (
    <div className="border-base-border bg-base-surface/30 hover:bg-base-elevated/40 group relative flex overflow-hidden rounded-lg border transition-all hover:shadow-md">
      {/* Left accent — status color */}
      <div className={`w-0.5 shrink-0 ${item.isLive ? 'bg-jade/50' : 'bg-rouge/40'}`} />

      <div className="flex min-w-0 flex-1 gap-3 p-3">
        {/* Identicon */}
        <CardLink className="shrink-0 self-start">
          <CellGlyph hash={item.nftId} size={36} />
        </CardLink>

        {/* Content */}
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          {/* Top row: primary label + status */}
          <div className="flex items-center justify-between gap-2">
            <CardLink className="hover:text-emphasis text-text-bright min-w-0 truncate font-mono text-sm font-semibold hover:underline">
              {primaryLabel || (
                <HexDisplay value={item.nftId} size="sm" startChars={8} endChars={6} />
              )}
            </CardLink>
            {item.isLive ? (
              <Badge variant="green">Live</Badge>
            ) : (
              <Badge variant="red">{inactiveStatusLabel(item)}</Badge>
            )}
          </div>

          {/* Secondary: show name if token number was primary, or compact ID */}
          {tokenIndex !== null && hasName ? (
            <div className="text-text truncate font-mono text-[11px]">{item.name}</div>
          ) : (
            <div className="text-text-dim font-mono text-[10px]">
              <HexDisplay value={item.nftId} size="sm" startChars={8} endChars={6} />
            </div>
          )}

          {/* Footer: owner + block */}
          <div className="mt-auto flex items-end justify-between gap-2 pt-0.5">
            {item.ownerLockHash ? (
              <Link
                href={`/address/${item.ownerLockHash}`}
                className="text-text-dim hover:text-text font-mono text-[10px] hover:underline"
              >
                <HexDisplay value={item.ownerLockHash} size="sm" startChars={6} endChars={4} />
              </Link>
            ) : (
              <span className="text-text-dim font-mono text-[10px]">{'\u2014'}</span>
            )}
            <Link
              href={`/blocks/${item.createdAtBlock}`}
              className="text-text-dim hover:text-emphasis shrink-0 font-mono text-[10px] hover:underline"
            >
              #{formatNumber(item.createdAtBlock)}
            </Link>
          </div>
        </div>
      </div>
    </div>
  );
}

/* ---------- Spore Object Card ---------- */

interface SporeObjectCardProps {
  spore: SporeNft;
  /** Resolved owner address (from parent lookup), if available */
  resolvedOwnerAddress?: string | null;
}

function SporeObjectCard({ spore, resolvedOwnerAddress }: SporeObjectCardProps) {
  const href = `/objects/${spore.sporeId}`;
  const icon = getContentTypeIcon(spore.contentType);
  const isLive = spore.isLive !== false;
  const ownerDisplay = resolvedOwnerAddress || spore.ownerAddress || null;

  return (
    <div className="border-base-border bg-base-surface/30 hover:bg-base-elevated/40 group relative flex overflow-hidden rounded-lg border transition-all hover:shadow-md">
      {/* Left accent — status color */}
      <div className={`w-0.5 shrink-0 ${isLive ? 'bg-jade/50' : 'bg-rouge/40'}`} />

      <div className="flex min-w-0 flex-1 gap-3 p-3">
        {/* Identicon */}
        <Link href={href} className="shrink-0 self-start">
          <CellGlyph hash={spore.sporeId} size={36} />
        </Link>

        {/* Content */}
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          {/* Top row: spore ID + status */}
          <div className="flex items-center justify-between gap-2">
            <Link
              href={href}
              className="hover:text-emphasis text-text-bright min-w-0 truncate font-mono text-sm font-semibold hover:underline"
            >
              <HexDisplay value={spore.sporeId} size="sm" startChars={8} endChars={6} />
            </Link>
            {isLive ? <Badge variant="green">Live</Badge> : <Badge variant="red">Burned</Badge>}
          </div>

          {/* Secondary: content type + size */}
          <div className="text-text-dim flex items-center gap-1.5 font-mono text-[10px]">
            <span>{icon}</span>
            <span className="truncate" title={spore.contentType}>
              {spore.contentType || 'unknown'}
            </span>
            <span className="text-text-dim/60">{'\u00B7'}</span>
            <span className="shrink-0">{formatNumber(spore.contentSize ?? 0)} B</span>
          </div>

          {/* Footer: owner + block */}
          <div className="mt-auto flex items-end justify-between gap-2 pt-0.5">
            {ownerDisplay ? (
              <Link
                href={`/address/${ownerDisplay}`}
                className="text-text-dim hover:text-text min-w-0 truncate font-mono text-[10px] hover:underline"
                title={ownerDisplay}
              >
                {ownerDisplay.length > 24
                  ? `${ownerDisplay.slice(0, 12)}...${ownerDisplay.slice(-8)}`
                  : ownerDisplay}
              </Link>
            ) : spore.ownerLockHash ? (
              <Link
                href={`/address/${spore.ownerLockHash}`}
                className="text-text-dim hover:text-text font-mono text-[10px] hover:underline"
              >
                <HexDisplay value={spore.ownerLockHash} size="sm" startChars={6} endChars={4} />
              </Link>
            ) : (
              <span className="text-text-dim font-mono text-[10px]">{'\u2014'}</span>
            )}
            <Link
              href={`/blocks/${spore.createdAtBlock}`}
              className="text-text-dim hover:text-emphasis shrink-0 font-mono text-[10px] hover:underline"
            >
              #{formatNumber(spore.createdAtBlock ?? 0)}
            </Link>
          </div>
        </div>
      </div>
    </div>
  );
}

/* ---------- Gallery Panel ---------- */

interface SporeItemEntry {
  spore: SporeNft;
  resolvedOwnerAddress?: string | null;
}

interface ObjectGalleryPanelBaseProps {
  className?: string;
  totalCount: number;
  isLoading: boolean;
  isError?: boolean;
  isFetching?: boolean;
  // Pagination
  page: number;
  hasPrevious: boolean;
  hasMore?: boolean;
  onNext: () => void;
  onPrevious: () => void;
  /** Custom actions rendered in the panel header (e.g. filter controls) */
  actions?: React.ReactNode;
}

interface CollectionItemGalleryProps extends ObjectGalleryPanelBaseProps {
  variant?: 'collection';
  collectionItems: CursorPaginatedResponse<CollectionItem> | undefined;
  sporeItems?: never;
  // Optional filters (dotbit/did:ckb collections)
  supportsFilters?: boolean;
  statusFilter?: ItemStatusFilter;
  onStatusFilterChange?: (status: ItemStatusFilter) => void;
  searchInput?: string;
  onSearchInputChange?: (value: string) => void;
  searchLabel?: string;
  inactiveStatusLabel?: string;
}

interface SporeItemGalleryProps extends ObjectGalleryPanelBaseProps {
  variant: 'spore';
  sporeItems: SporeItemEntry[];
  collectionItems?: never;
  supportsFilters?: never;
  statusFilter?: never;
  onStatusFilterChange?: never;
  searchInput?: never;
  onSearchInputChange?: never;
  searchLabel?: never;
  inactiveStatusLabel?: never;
}

type ObjectGalleryPanelProps = CollectionItemGalleryProps | SporeItemGalleryProps;

export type { SporeItemEntry };

export function ObjectGalleryPanel(props: ObjectGalleryPanelProps) {
  const {
    className,
    totalCount,
    isLoading,
    isError,
    isFetching,
    page,
    hasPrevious,
    hasMore: hasMoreProp,
    onNext,
    onPrevious,
    actions,
  } = props;

  const isSporeVariant = props.variant === 'spore';

  // Determine item count and hasMore based on variant
  const itemCount = isSporeVariant
    ? props.sporeItems.length
    : (props.collectionItems?.data?.length ?? 0);
  const hasData = itemCount > 0;
  const hasMore =
    hasMoreProp ?? (isSporeVariant ? false : (props.collectionItems?.hasMore ?? false));
  const total = isSporeVariant ? totalCount : (props.collectionItems?.total ?? totalCount);

  // Default header actions for collection variant (built-in filters)
  const defaultCollectionActions =
    !isSporeVariant &&
    props.supportsFilters &&
    props.onStatusFilterChange &&
    props.onSearchInputChange ? (
      <>
        <select
          value={props.statusFilter}
          onChange={(e) => props.onStatusFilterChange!(e.target.value as ItemStatusFilter)}
          aria-label="Status Filter"
          className="focus:border-emphasis border-base-border bg-base-surface text-text-bright rounded border px-2.5 py-1.5 font-mono text-xs outline-none transition-colors"
        >
          <option value="all">All</option>
          <option value="live">Live</option>
          <option value="recycled">{props.inactiveStatusLabel || 'Recycled'}</option>
        </select>
        <input
          type="text"
          value={props.searchInput ?? ''}
          onChange={(e) => props.onSearchInputChange!(e.target.value)}
          placeholder={props.searchLabel || 'Search...'}
          aria-label={props.searchLabel || 'Search'}
          className="focus:border-emphasis border-base-border bg-base-surface text-text-bright placeholder:text-text-dim w-44 rounded border px-2.5 py-1.5 font-mono text-xs outline-none transition-colors"
        />
        {isFetching && <span className="text-text-dim font-mono text-xs">Searching...</span>}
      </>
    ) : null;

  const headerActions = actions || defaultCollectionActions;

  return (
    <TerminalPanel className={className}>
      <TerminalPanelHeader
        indicator="active"
        actions={
          headerActions ? (
            <div className="flex flex-wrap items-center gap-2">{headerActions}</div>
          ) : undefined
        }
      >
        Objects ({formatNumber(totalCount)})
      </TerminalPanelHeader>

      <TerminalPanelContent>
        {isLoading && itemCount === 0 ? (
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
            {Array.from({ length: 6 }).map((_, i) => (
              <div
                key={i}
                className="border-base-border bg-base-surface/30 h-24 animate-pulse rounded-lg border"
              />
            ))}
          </div>
        ) : isError ? (
          <div className="text-rouge py-8 text-center">
            Failed to load objects. Please refresh and try again.
          </div>
        ) : !hasData ? (
          <div className="text-text-dim py-8 text-center">No objects in this collection</div>
        ) : (
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
            {isSporeVariant
              ? props.sporeItems.map((entry) => (
                  <SporeObjectCard
                    key={entry.spore.sporeId}
                    spore={entry.spore}
                    resolvedOwnerAddress={entry.resolvedOwnerAddress}
                  />
                ))
              : props.collectionItems!.data.map((item) => (
                  <ObjectCard key={item.nftId} item={item} />
                ))}
          </div>
        )}
      </TerminalPanelContent>

      {hasData && (
        <TerminalPanelFooter>
          <CursorPagination
            total={total}
            totalLabel="Objects"
            pageSize={GALLERY_PAGE_SIZE}
            page={page}
            currentCount={itemCount}
            hasMore={hasMore}
            hasPrevious={hasPrevious}
            onNext={onNext}
            onPrevious={onPrevious}
          />
        </TerminalPanelFooter>
      )}
    </TerminalPanel>
  );
}
