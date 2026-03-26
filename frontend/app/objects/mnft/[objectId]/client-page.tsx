'use client';
import { useCallback, useEffect, useMemo, useState } from 'react';
import Link from '@/components/ui/link';
import { usePathname, useRouter, useSearchParams } from '@/src/navigation';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { Header } from '@/components/layout/header';
import { ObjectActivityCard } from '@/components/object/object-activity-card';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import {
  TerminalPanel,
  TerminalPanelContent,
  TerminalPanelFooter,
  TerminalPanelHeader,
} from '@/components/ui/terminal-panel';
import { Badge } from '@/components/ui/page-header';
import { HexDisplay } from '@/components/ui/hex-display';
import { Address } from '@/components/ui/address';
import { api } from '@/lib/api';
import { formatCompositionTier, normalizeAssetId, parseActivityCursor } from '@/lib/asset-utils';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
import { formatNumber } from '@/lib/utils';
import { Tooltip } from '@/components/spore/cluster-description';
function decodeTokenState(state: number): string {
  switch (state) {
    case 0:
      return 'normal';
    case 1:
      return 'locked';
    case 2:
      return 'frozen';
    default:
      return `unknown(${state})`;
  }
}
function decodeTokenConfigure(configure: number): string {
  const flags: string[] = [];
  if ((configure & 0b00000001) !== 0) flags.push('transferable');
  if ((configure & 0b00000010) !== 0) flags.push('burnable');
  if ((configure & 0b00000100) !== 0) flags.push('mutable');
  if ((configure & 0b00001000) !== 0) flags.push('reserved_3');
  return flags.length > 0 ? flags.join(', ') : 'none';
}

const COMPOSITION_TIER_DESCRIPTIONS: Record<string, string> = {
  pure_ckb:
    'All content is stored directly on the CKB blockchain (on-chain data or ckbfs://). Fully verifiable and permanent.',
  btc_ckb:
    'Content is stored across both CKB (on-chain data or ckbfs://) and Bitcoin (btcfs://). Fully verifiable and permanent.',
  decentralized_mixture:
    'Some content references external decentralized storage (e.g. IPFS, Arweave). Data persists as long as the external network hosts it.',
  centralized_mixture:
    'Some content depends on centralized servers (http/https). Data availability relies on the server operator.',
  unknown:
    'Composition could not be determined. The content storage method for items in this collection is unverified.',
};

const TOOLTIP_BTN_BASE =
  'ml-1 inline-flex h-3.5 w-3.5 items-center justify-center rounded-full border font-mono text-[9px] leading-none transition-colors';

function compositionTierCardStyle(tier: string): {
  card: string;
  label: string;
  text: string;
  tooltipButton?: string;
} {
  if (tier === 'btc_ckb') {
    return {
      card: 'storage-card-no-crt storage-card-both rounded border border-[#222840] bg-[#10131c] p-3',
      label: 'text-[#a0b880]',
      text: 'storage-text-split',
      tooltipButton: `${TOOLTIP_BTN_BASE} text-[#a0b880] border-[#4a6838] hover:text-[#c0d8a0] hover:border-[#6a8850]`,
    };
  }
  if (tier === 'pure_ckb' || tier === 'fully_onchain') {
    return {
      card: 'storage-card-no-crt storage-card-ckb rounded border border-[#222840] bg-[#10131c] p-3',
      label: 'text-[#5abfa0]',
      text: 'storage-text-gem',
      tooltipButton: `${TOOLTIP_BTN_BASE} text-[#5abfa0] border-[#1a6050] hover:text-[#40e8b0] hover:border-[#2a8068]`,
    };
  }
  if (tier === 'centralized_mixture') {
    return {
      card: 'border-base-border rounded border p-3',
      label: 'text-text-dim',
      text: 'text-negative',
    };
  }
  return {
    card: 'border-base-border rounded border p-3',
    label: 'text-text-dim',
    text: 'text-warning',
  };
}

function CompositionTierTooltip({
  tier,
  buttonClassName,
}: {
  tier: string;
  buttonClassName?: string;
}) {
  const text = COMPOSITION_TIER_DESCRIPTIONS[tier] || COMPOSITION_TIER_DESCRIPTIONS.unknown;
  return <Tooltip text={text} buttonClassName={buttonClassName} />;
}

function parseHexToBytes(hex: string): Uint8Array {
  const clean = hex.startsWith('0x') ? hex.slice(2) : hex;
  const bytes = new Uint8Array(clean.length / 2);
  for (let i = 0; i < bytes.length; i++) {
    bytes[i] = parseInt(clean.slice(i * 2, i * 2 + 2), 16);
  }
  return bytes;
}

export interface MnftItemDetailPageProps {
  objectId: string;
}
export default function MnftItemDetailPage({ objectId: routeObjectId }: MnftItemDetailPageProps) {
  const pathname = usePathname();
  const router = useRouter();
  const searchParams = useSearchParams();
  const nftId = normalizeAssetId(routeObjectId);
  const [activityCursor, setActivityCursor] = useState<string | undefined>(() =>
    parseActivityCursor(searchParams.get('activity_cursor'))
  );
  const [activityCursorHistory, setActivityCursorHistory] = useState<string[]>([]);
  const [hoveredByteOffset, setHoveredByteOffset] = useState<number | null>(null);
  const detailQuery = useQuery({
    queryKey: ['mnft-item-detail', nftId],
    queryFn: () => api.getMnftItemDetail(nftId),
    retry: false,
  });
  const detail = detailQuery.data;
  const { data: ownerAddressRecord } = useQuery({
    queryKey: ['address-by-lock-hash', detail?.ownerLockHash],
    queryFn: () => api.getAddress(detail!.ownerLockHash!),
    enabled: !!detail?.ownerLockHash,
    retry: false,
  });
  const { data: itemActivities, isLoading: isActivitiesLoading } = useQuery({
    queryKey: ['mnft-item-activities', detail?.nftId, activityCursor],
    queryFn: () => {
      const queryParams: { limit: number; cursor?: string } = { limit: DEFAULT_PAGE_SIZE };
      if (activityCursor) {
        queryParams.cursor = activityCursor;
      }
      return api.getMnftItemActivities(detail!.nftId, queryParams);
    },
    enabled: !!detail?.nftId,
    retry: false,
    placeholderData: keepPreviousData,
  });
  const payloadBytes = useMemo(() => {
    if (!detail?.characteristicHex) return null;
    const clean = detail.characteristicHex.startsWith('0x')
      ? detail.characteristicHex.slice(2)
      : detail.characteristicHex;
    if (clean.length === 0) return null;
    return parseHexToBytes(clean);
  }, [detail?.characteristicHex]);
  const payloadHex = useMemo(() => {
    if (!detail?.characteristicHex) return '';
    return detail.characteristicHex.startsWith('0x')
      ? detail.characteristicHex.slice(2)
      : detail.characteristicHex;
  }, [detail?.characteristicHex]);
  const goToNextActivityPage = useCallback(
    (nextCursor: string | null | undefined) => {
      if (!nextCursor) return;
      setActivityCursorHistory((prev) => [...prev, activityCursor || '']);
      setActivityCursor(nextCursor);
    },
    [activityCursor]
  );
  const goToPreviousActivityPage = useCallback(() => {
    if (activityCursorHistory.length === 0) return;
    const prev = activityCursorHistory[activityCursorHistory.length - 1];
    setActivityCursorHistory((history) => history.slice(0, -1));
    setActivityCursor(prev || undefined);
  }, [activityCursorHistory]);
  useEffect(() => {
    const nextParams = new URLSearchParams(searchParams.toString());
    if (activityCursor) {
      nextParams.set('activity_cursor', activityCursor);
    } else {
      nextParams.delete('activity_cursor');
    }
    const current = searchParams.toString();
    const next = nextParams.toString();
    if (next === current) return;
    router.replace(next ? `${pathname}?${next}` : pathname, { scroll: false });
  }, [activityCursor, pathname, router, searchParams]);
  if (detailQuery.isLoading) {
    return (
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <div className="bg-base-elevated mb-6 h-10 w-48 animate-pulse rounded" />
          <div className="space-y-6">
            <div className="border-base-border bg-base-surface/40 h-40 animate-pulse rounded border" />
            <div className="border-base-border bg-base-surface/40 h-64 animate-pulse rounded border" />
          </div>
        </main>
      </div>
    );
  }
  if (!detail) {
    return (
      <div className="bg-base-bg min-h-screen">
        <Header />
        <main className="container mx-auto px-4 py-8">
          <TerminalPanel>
            <TerminalPanelContent className="py-12 text-center">
              <h2 className="text-text-dim text-xl">mNFT item not found</h2>
            </TerminalPanelContent>
          </TerminalPanel>
        </main>
      </div>
    );
  }
  const ownerAddress = ownerAddressRecord?.address || null;
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">
        {/* Breadcrumb */}
        <nav className="text-text-dim mb-6 flex items-center gap-1.5 font-mono text-sm">
          <Link href="/inventory/objects" className="hover:text-emphasis transition-colors">
            Objects
          </Link>
          <span>&rsaquo;</span>
          <Link
            href={`/classes/${detail.class.classId}`}
            className="hover:text-emphasis transition-colors"
          >
            {detail.class.name || 'Class'}
          </Link>
          <span>&rsaquo;</span>
          <span className="text-text">#{formatNumber(detail.tokenIndex)}</span>
        </nav>

        {/* mNFT Overview */}
        <TerminalPanel className="mb-6">
          <TerminalPanelHeader indicator="active">mNFT Overview</TerminalPanelHeader>
          <TerminalPanelContent>
            {/* Title + badge */}
            <div className="flex flex-wrap items-center gap-3">
              <h1 className="text-text-bright font-mono text-2xl font-bold">
                {detail.class.name
                  ? `${detail.class.name} #${formatNumber(detail.tokenIndex)}`
                  : `mNFT #${formatNumber(detail.tokenIndex)}`}
              </h1>
              {detail.isLive ? (
                <Badge variant="green">Live</Badge>
              ) : (
                <Badge variant="red">Burned</Badge>
              )}
            </div>

            {/* Token ID */}
            <div className="mt-3 flex flex-wrap items-baseline gap-2 font-mono text-sm">
              <span className="text-text-dim text-xs uppercase tracking-wider">token id</span>
              <HexDisplay value={detail.nftId} truncate={false} size="sm" />
            </div>

            {/* Cell */}
            <div className="mt-1.5 flex flex-wrap items-baseline gap-2 font-mono text-sm">
              <span className="text-text-dim text-xs uppercase tracking-wider">cell</span>
              {detail.txHash !== null && detail.outputIndex !== null ? (
                <Link
                  href={`/cell/${detail.txHash}-${detail.outputIndex}`}
                  className="hover:underline"
                >
                  <HexDisplay value={detail.txHash} size="sm" startChars={14} endChars={10} />
                  <span className="text-text-dim">-{detail.outputIndex}</span>
                </Link>
              ) : (
                <span className="text-text-dim">no live outpoint</span>
              )}
            </div>

            {/* Owner */}
            <div className="mt-1.5 flex flex-wrap items-baseline gap-2 font-mono text-sm">
              <span className="text-text-dim text-xs uppercase tracking-wider">owner</span>
              {ownerAddress ? (
                <Address address={ownerAddress} truncate={false} />
              ) : detail.ownerLockHash ? (
                <Link href={`/address/${detail.ownerLockHash}`} className="hover:underline">
                  <HexDisplay
                    value={detail.ownerLockHash}
                    size="sm"
                    startChars={14}
                    endChars={10}
                  />
                </Link>
              ) : (
                <span className="text-text-dim">unavailable</span>
              )}
            </div>

            {/* Stat cards row */}
            <div className="border-base-border mt-4 grid grid-cols-2 gap-3 border-t pt-4 sm:grid-cols-4">
              {/* Composition card */}
              {detail.composition?.tier &&
                (() => {
                  const style = compositionTierCardStyle(detail.composition.tier);
                  return (
                    <div className={style.card}>
                      <div
                        className={`mb-1.5 font-mono text-[10px] uppercase tracking-wider ${style.label}`}
                      >
                        Composition
                      </div>
                      <div className="flex items-center gap-1">
                        <span
                          className={`font-mono text-sm font-semibold leading-tight ${style.text}`}
                        >
                          {formatCompositionTier(detail.composition.tier)}
                        </span>
                        <CompositionTierTooltip
                          tier={detail.composition.tier}
                          buttonClassName={style.tooltipButton}
                        />
                      </div>
                    </div>
                  );
                })()}

              {/* Class card */}
              <div className="border-base-border rounded border p-3">
                <div className="text-text-dim mb-1.5 font-mono text-[10px] uppercase tracking-wider">
                  Class
                </div>
                <Link
                  href={`/classes/${detail.class.classId}`}
                  className="text-text-bright font-mono text-sm font-semibold hover:underline"
                >
                  {detail.class.name || 'Unnamed'}
                </Link>
                <div className="text-text-dim font-mono text-xs">
                  {formatNumber(detail.class.issued)} / {formatNumber(detail.class.total)} issued
                </div>
              </div>

              {/* Issuer card */}
              <div className="border-base-border rounded border p-3">
                <div className="text-text-dim mb-1.5 font-mono text-[10px] uppercase tracking-wider">
                  Issuer
                </div>
                <div className="text-text-bright font-mono text-sm font-semibold">
                  {detail.issuer.name || 'Unnamed'}
                </div>
                <div className="text-text-dim font-mono text-xs">
                  {formatNumber(detail.issuer.classCount)} classes &middot;{' '}
                  {formatNumber(detail.issuer.setCount)} sets
                </div>
              </div>

              {/* Created card */}
              <div className="border-base-border rounded border p-3">
                <div className="text-text-dim mb-1.5 font-mono text-[10px] uppercase tracking-wider">
                  Created
                </div>
                <Link
                  href={`/blocks/${detail.createdAtBlock}`}
                  className="text-text-bright font-mono text-sm font-semibold tabular-nums hover:underline"
                >
                  #{formatNumber(detail.createdAtBlock)}
                </Link>
              </div>
            </div>

            {/* Collection info */}
            {(detail.class.description || detail.class.renderer) && (
              <div className="border-base-border mt-3 space-y-1.5 border-t pt-3">
                {detail.class.description && (
                  <div className="text-text-dim text-xs leading-relaxed">
                    {detail.class.description}
                  </div>
                )}
                {detail.class.renderer && (
                  <div className="text-text-dim font-mono text-xs">
                    renderer: {detail.class.renderer}
                  </div>
                )}
              </div>
            )}
          </TerminalPanelContent>
        </TerminalPanel>

        {/* Properties */}
        <TerminalPanel className="mb-6">
          <TerminalPanelHeader indicator="active">Properties</TerminalPanelHeader>
          <TerminalPanelContent>
            <div className="space-y-2">
              <div className="flex flex-wrap items-baseline gap-2 font-mono text-sm">
                <span className="text-text-dim text-xs uppercase tracking-wider">state</span>
                <span className="text-text-bright">{decodeTokenState(detail.state)}</span>
              </div>
              <div className="flex flex-wrap items-baseline gap-2 font-mono text-sm">
                <span className="text-text-dim text-xs uppercase tracking-wider">configure</span>
                <span className="text-text-bright">{decodeTokenConfigure(detail.configure)}</span>
              </div>
              {detail.characteristicHex && (
                <div className="flex flex-wrap items-baseline gap-2 font-mono text-sm">
                  <span className="text-text-dim text-xs uppercase tracking-wider">
                    characteristic
                  </span>
                  <HexDisplay value={detail.characteristicHex} truncate={false} size="sm" />
                </div>
              )}
            </div>
          </TerminalPanelContent>
        </TerminalPanel>

        {/* Payload Data hex viewer */}
        {payloadBytes && payloadBytes.length > 0 && (
          <TerminalPanel className="mb-6">
            <TerminalPanelHeader indicator="active">
              Payload Data ({formatNumber(payloadBytes.length)} bytes)
            </TerminalPanelHeader>
            <TerminalPanelContent>
              <div className="border-base-border bg-base-bg overflow-x-auto rounded-md border p-4 font-mono text-xs">
                <div className="min-w-max" onMouseLeave={() => setHoveredByteOffset(null)}>
                  {(() => {
                    const BYTES_PER_ROW = 16;
                    const MAX_BYTES = 512;
                    const totalBytes = payloadBytes.length;
                    const displayBytes = Math.min(totalBytes, MAX_BYTES);
                    const rows = [];
                    for (let r = 0; r < displayBytes; r += BYTES_PER_ROW) {
                      const end = Math.min(r + BYTES_PER_ROW, displayBytes);
                      const rowBytes: { hex: string; ascii: string; offset: number }[] = [];
                      for (let i = r; i < end; i++) {
                        const h = payloadHex.slice(i * 2, i * 2 + 2);
                        const code = payloadBytes[i];
                        const ch = code >= 32 && code <= 126 ? String.fromCharCode(code) : '.';
                        rowBytes.push({ hex: h, ascii: ch, offset: i });
                      }
                      rows.push({ offset: r, bytes: rowBytes });
                    }
                    return (
                      <>
                        {rows.map((row) => {
                          const padCount = BYTES_PER_ROW - row.bytes.length;
                          return (
                            <div key={row.offset} className="hover:bg-base-elevated/50 flex py-0.5">
                              <span className="text-text-dim mr-4 select-none">
                                0x{row.offset.toString(16).padStart(4, '0')}:
                              </span>
                              <div className="text-emphasis-dim mr-6 flex gap-1.5">
                                {row.bytes.map((b) => (
                                  <span
                                    key={b.offset}
                                    className={
                                      hoveredByteOffset === b.offset
                                        ? 'bg-emphasis/25 text-emphasis ring-emphasis/70 rounded ring-1'
                                        : 'bg-base-elevated/70 text-text rounded'
                                    }
                                    onMouseEnter={() => setHoveredByteOffset(b.offset)}
                                  >
                                    {b.hex}
                                  </span>
                                ))}
                                {Array.from({ length: padCount }).map((_, i) => (
                                  <span key={`pad-${i}`} className="opacity-0">
                                    00
                                  </span>
                                ))}
                              </div>
                              <div className="border-base-border text-text-dim border-l pl-4">
                                {row.bytes.map((b) => (
                                  <span
                                    key={`a-${b.offset}`}
                                    className={`inline-flex w-2.5 justify-center ${
                                      hoveredByteOffset === b.offset
                                        ? 'bg-emphasis/20 text-emphasis rounded-sm'
                                        : ''
                                    }`}
                                    onMouseEnter={() => setHoveredByteOffset(b.offset)}
                                  >
                                    {b.ascii}
                                  </span>
                                ))}
                              </div>
                            </div>
                          );
                        })}
                        {totalBytes > MAX_BYTES && (
                          <div className="text-text-dim mt-2 select-none italic">
                            ... {(totalBytes - MAX_BYTES).toLocaleString()} more bytes
                          </div>
                        )}
                      </>
                    );
                  })()}
                </div>
              </div>
            </TerminalPanelContent>
          </TerminalPanel>
        )}

        {/* Activities */}
        <TerminalPanel>
          <TerminalPanelHeader indicator="active">Activities</TerminalPanelHeader>
          <TerminalPanelContent>
            {isActivitiesLoading ? (
              <div className="text-text-dim py-2 text-sm">Loading activities...</div>
            ) : !itemActivities?.data?.length ? (
              <div className="text-text-dim py-2 text-sm">No related activities found.</div>
            ) : (
              <div className="space-y-2">
                {itemActivities.data.map((activity) => (
                  <ObjectActivityCard
                    key={`${activity.blockNumber}-${activity.txIndex}-${activity.txHash}`}
                    txHash={activity.txHash}
                    blockNumber={activity.blockNumber}
                    txIndex={activity.txIndex}
                    actions={activity.actions}
                    badgeActions
                  />
                ))}
              </div>
            )}
          </TerminalPanelContent>
          <TerminalPanelFooter>
            <CursorPagination
              total={itemActivities?.total ?? undefined}
              totalLabel="activities"
              pageSize={DEFAULT_PAGE_SIZE}
              page={activityCursorHistory.length + 1}
              currentCount={itemActivities?.data?.length ?? 0}
              hasMore={itemActivities?.hasMore ?? false}
              hasPrevious={activityCursorHistory.length > 0}
              onNext={() => goToNextActivityPage(itemActivities?.nextCursor)}
              onPrevious={goToPreviousActivityPage}
            />
          </TerminalPanelFooter>
        </TerminalPanel>
      </main>
    </div>
  );
}
