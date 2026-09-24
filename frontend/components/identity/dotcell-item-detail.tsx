'use client';

import Link from '@/components/ui/link';
import { usePathname, useRouter, useSearchParams } from '@/src/navigation';
import { useQuery } from '@tanstack/react-query';
import { useCallback, useEffect, useState } from 'react';

import { Header } from '@/components/layout/header';
import { DotCellParty } from '@/components/identity/dotcell-party';
import { IdentityActivityCard } from '@/components/identity/identity-activity-card';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { DataField, DataGrid } from '@/components/ui/data-field';
import { HexDisplay } from '@/components/ui/hex-display';
import { PageHeader, Badge } from '@/components/ui/page-header';
import {
  TerminalPanel,
  TerminalPanelContent,
  TerminalPanelFooter,
  TerminalPanelHeader,
} from '@/components/ui/terminal-panel';
import { api, type DotCellOutPoint, type DotCellState } from '@/lib/api';
import {
  formatActivityTimestamp,
  formatExpiry,
  normalizeActivityAction,
  parseActivityCursor,
} from '@/lib/asset-utils';
import { getIdentityItemDetailHref } from '@/lib/detail-routes';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';
import { formatCapacity, formatNumber } from '@/lib/utils';

interface Props {
  /** A 20-byte `0x…` name id, or a name (`alice` / `alice.cell`). */
  identityId: string;
}

const STATE_VARIANT: Record<DotCellState, 'green' | 'gold' | 'gray' | 'red'> = {
  active: 'green',
  grace: 'gold',
  free: 'gray',
  recycled: 'red',
};

function cellHref(outPoint: DotCellOutPoint): string {
  return `/cell/${outPoint.txHash}-${outPoint.index}`;
}

function nameHref(identityId: string): string {
  return getIdentityItemDetailHref('dotcell', identityId);
}

export function DotCellItemDetail({ identityId: routeIdentityId }: Props) {
  // A `.cell` reference may be an id or a name, and the API accepts both, so
  // the route value goes to the API untouched.
  const itemRef = decodeURIComponent(routeIdentityId);
  // Parent, sub-name and ring-successor links change only the route parameter,
  // so the router keeps this element mounted. Keying on the name gives each one
  // its own activity paging instead of inheriting the previous name's cursor.
  return <DotCellItemDetailView key={itemRef} itemRef={itemRef} />;
}

function DotCellItemDetailView({ itemRef }: { itemRef: string }) {
  const pathname = usePathname();
  const router = useRouter();
  const searchParams = useSearchParams();
  const [activityCursor, setActivityCursor] = useState<string | undefined>(() =>
    parseActivityCursor(searchParams.get('activity_cursor'))
  );
  const [activityCursorHistory, setActivityCursorHistory] = useState<string[]>([]);

  const detailQuery = useQuery({
    queryKey: ['dotcell-item-detail', itemRef],
    queryFn: () => api.getDotCellItemDetail(itemRef),
    retry: false,
  });

  const detail = detailQuery.data;

  const ringQuery = useQuery({
    queryKey: ['dotcell-ring'],
    queryFn: () => api.getDotCellRing(),
    retry: false,
  });

  const { data: itemActivities, isLoading: isActivitiesLoading } = useQuery({
    queryKey: ['dotcell-item-activities', detail?.identityId, activityCursor],
    queryFn: () => {
      const queryParams: { limit: number; cursor?: string } = { limit: DEFAULT_PAGE_SIZE };
      if (activityCursor) {
        queryParams.cursor = activityCursor;
      }
      return api.getDotCellItemActivities(detail!.identityId, queryParams);
    },
    enabled: !!detail?.identityId,
    retry: false,
  });

  const goToNextActivityPage = useCallback(
    (nextCursor: string | null | undefined) => {
      if (!nextCursor) {
        return;
      }
      setActivityCursorHistory((prev) => [...prev, activityCursor || '']);
      setActivityCursor(nextCursor);
    },
    [activityCursor]
  );

  const goToPreviousActivityPage = useCallback(() => {
    if (activityCursorHistory.length === 0) {
      return;
    }
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
    if (next === current) {
      return;
    }
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
            <div className="border-base-border bg-base-surface/40 h-52 animate-pulse rounded border" />
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
              <h2 className="text-text-dim text-xl">.cell name not found</h2>
            </TerminalPanelContent>
          </TerminalPanel>
        </main>
      </div>
    );
  }

  const ring = ringQuery.data;

  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <div className="mb-6 flex items-center gap-4">
          <Link
            href="/identities/dotcell"
            className="hover:text-emphasis text-text-dim text-sm transition-colors"
          >
            ← Back to .cell Collection
          </Link>
          <Link
            href="/identities"
            className="hover:text-emphasis text-text-dim text-sm transition-colors"
          >
            Back to Identities
          </Link>
        </div>

        <div className="mb-6">
          <PageHeader
            title={detail.name}
            badge={<Badge variant={STATE_VARIANT[detail.state]}>{detail.state}</Badge>}
            className="mb-0"
          />
        </div>

        <div className="space-y-6">
          <TerminalPanel>
            <TerminalPanelHeader indicator="active">Name</TerminalPanelHeader>
            <TerminalPanelContent>
              <DataGrid columns={2}>
                <DataField label="Label">
                  <span className="text-text-bright font-mono">{detail.label}</span>
                </DataField>
                <DataField label="Name">
                  <span className="text-text-bright font-mono">{detail.name}</span>
                </DataField>
                <DataField label="State">
                  <Badge variant={STATE_VARIANT[detail.state]}>{detail.state}</Badge>
                </DataField>
                <DataField label="Layout Version">
                  <span className="text-text-bright font-mono">v{detail.layoutVersion}</span>
                </DataField>
                <DataField label="Expires At">
                  <span className="text-text-bright font-mono">
                    {formatExpiry(detail.expiredAt)}
                  </span>
                </DataField>
                <DataField label="Grace Ends At">
                  <span className="text-text-bright font-mono">
                    {formatExpiry(detail.graceEndsAt)}
                  </span>
                </DataField>
                <DataField label="Name ID" layout="vertical" valueClassName="w-full">
                  <HexDisplay value={detail.identityId} truncate={false} />
                </DataField>
                <DataField label="Namespace Args" layout="vertical" valueClassName="w-full">
                  <HexDisplay value={detail.namespaceArgs} truncate={false} />
                </DataField>
              </DataGrid>
            </TerminalPanelContent>
          </TerminalPanel>

          <TerminalPanel>
            <TerminalPanelHeader indicator="active">Ownership</TerminalPanelHeader>
            <TerminalPanelContent>
              <DataGrid columns={1}>
                <DataField
                  label="Owner"
                  layout="vertical"
                  valueClassName="w-full"
                  helpText="The chain stores only the first 20 bytes of the owner's lock script hash. An unresolved prefix names no address."
                >
                  <DotCellParty party={detail.owner} testId="dotcell-owner" />
                </DataField>
                <DataField label="Manager" layout="vertical" valueClassName="w-full">
                  <DotCellParty party={detail.manager} testId="dotcell-manager" />
                </DataField>
              </DataGrid>
            </TerminalPanelContent>
          </TerminalPanel>

          {detail.sale && (
            <TerminalPanel data-testid="dotcell-sale">
              <TerminalPanelHeader indicator="active">Sale</TerminalPanelHeader>
              <TerminalPanelContent>
                <DataGrid columns={1}>
                  <DataField label="Price">
                    <span className="text-text-bright font-mono">
                      {formatCapacity(detail.sale.priceShannons)}
                    </span>
                  </DataField>
                  <DataField label="Seller" layout="vertical" valueClassName="w-full">
                    <DotCellParty party={detail.sale.seller} testId="dotcell-seller" />
                  </DataField>
                  <DataField label="Offer Cell" layout="vertical" valueClassName="w-full">
                    {detail.sale.offerOutPoint ? (
                      <Link
                        href={cellHref(detail.sale.offerOutPoint)}
                        className="text-emphasis font-mono hover:underline"
                      >
                        <HexDisplay value={detail.sale.offerOutPoint.txHash} size="sm" />-
                        {detail.sale.offerOutPoint.index}
                      </Link>
                    ) : (
                      <span className="text-text-dim font-mono text-sm">No offer cell.</span>
                    )}
                  </DataField>
                </DataGrid>
              </TerminalPanelContent>
            </TerminalPanel>
          )}

          <TerminalPanel data-testid="dotcell-records">
            <TerminalPanelHeader indicator="active">Records</TerminalPanelHeader>
            <TerminalPanelContent padding="none">
              {detail.records.length === 0 ? (
                <div className="text-text-dim p-4 font-mono text-sm">No records set.</div>
              ) : (
                <div className="overflow-x-auto">
                  <table className="w-full text-left font-mono text-sm">
                    <thead>
                      <tr className="border-base-border text-text-dim border-b text-xs uppercase tracking-wider">
                        <th className="px-4 py-2 font-normal">Key</th>
                        <th className="px-4 py-2 font-normal">Label</th>
                        <th className="px-4 py-2 font-normal">Value</th>
                        <th className="px-4 py-2 text-right font-normal">TTL</th>
                      </tr>
                    </thead>
                    <tbody>
                      {detail.records.map((record, index) => (
                        <tr
                          key={`${record.key}-${record.label}-${index}`}
                          className="border-base-border border-b last:border-b-0"
                        >
                          <td className="text-text-bright px-4 py-2 align-top">{record.key}</td>
                          <td className="text-text-dim px-4 py-2 align-top">{record.label}</td>
                          <td className="text-text break-all px-4 py-2 align-top">
                            {record.decodedAddress ? (
                              <Link
                                href={`/address/${record.decodedAddress.address}`}
                                className="text-aqua hover:underline"
                              >
                                {record.decodedAddress.address}
                              </Link>
                            ) : (
                              // A record whose bytes are not UTF-8 shows its
                              // hex; nothing is guessed on its behalf.
                              (record.valueUtf8 ?? record.valueHex)
                            )}
                          </td>
                          <td className="text-text-dim px-4 py-2 text-right align-top">
                            {record.ttl}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </TerminalPanelContent>
            <TerminalPanelFooter>
              <div className="text-text-dim flex min-w-0 items-center gap-2 font-mono text-xs">
                <span className="uppercase tracking-wider">Records Hash</span>
                <HexDisplay value={detail.recordsHash} size="sm" />
              </div>
            </TerminalPanelFooter>
          </TerminalPanel>

          <TerminalPanel data-testid="dotcell-ring">
            <TerminalPanelHeader indicator="active">Ring</TerminalPanelHeader>
            <TerminalPanelContent>
              <DataGrid columns={2}>
                <DataField label="Next Name" layout="vertical" valueClassName="w-full">
                  <Link
                    href={nameHref(detail.nextId)}
                    className="text-emphasis font-mono hover:underline"
                  >
                    <HexDisplay value={detail.nextId} size="sm" />
                  </Link>
                </DataField>
                {ring && (
                  <DataField label="Live Names">
                    <span className="text-text-bright font-mono">
                      {formatNumber(ring.liveCount)}
                    </span>
                  </DataField>
                )}
                {ring && (
                  <DataField label="Ring Root" layout="vertical" valueClassName="w-full">
                    <Link
                      href={cellHref(ring.rootOutPoint)}
                      className="text-emphasis font-mono hover:underline"
                    >
                      <HexDisplay value={ring.rootOutPoint.txHash} size="sm" />-
                      {ring.rootOutPoint.index}
                    </Link>
                  </DataField>
                )}
                {ring && (
                  <DataField label="First Name" layout="vertical" valueClassName="w-full">
                    <Link
                      href={nameHref(ring.firstId)}
                      className="text-emphasis font-mono hover:underline"
                    >
                      <HexDisplay value={ring.firstId} size="sm" />
                    </Link>
                  </DataField>
                )}
              </DataGrid>
            </TerminalPanelContent>
          </TerminalPanel>

          <TerminalPanel data-testid="dotcell-subnames">
            <TerminalPanelHeader indicator="active">Sub-names</TerminalPanelHeader>
            <TerminalPanelContent>
              {!detail.parent && detail.children.length === 0 ? (
                <div className="text-text-dim font-mono text-sm">No sub-names.</div>
              ) : (
                <DataGrid columns={1}>
                  {detail.parent && (
                    <DataField label="Parent">
                      <Link
                        href={nameHref(detail.parent.identityId)}
                        className="text-emphasis font-mono hover:underline"
                      >
                        {detail.parent.name}
                      </Link>
                    </DataField>
                  )}
                  {detail.children.length > 0 && (
                    <DataField
                      label={`Children (${detail.children.length})`}
                      layout="vertical"
                      valueClassName="w-full"
                    >
                      <ul className="space-y-1">
                        {detail.children.map((child) => (
                          <li key={child.identityId}>
                            <Link
                              href={nameHref(child.identityId)}
                              className="text-emphasis font-mono hover:underline"
                            >
                              {child.name}
                            </Link>
                          </li>
                        ))}
                      </ul>
                    </DataField>
                  )}
                </DataGrid>
              )}
            </TerminalPanelContent>
          </TerminalPanel>

          <TerminalPanel>
            <TerminalPanelHeader indicator="active">Cell</TerminalPanelHeader>
            <TerminalPanelContent>
              <DataGrid columns={1}>
                <DataField label="Live Cell" layout="vertical" valueClassName="w-full">
                  {detail.liveOutPoint ? (
                    <Link
                      href={cellHref(detail.liveOutPoint)}
                      className="text-emphasis font-mono hover:underline"
                    >
                      <HexDisplay value={detail.liveOutPoint.txHash} size="sm" />-
                      {detail.liveOutPoint.index}
                    </Link>
                  ) : (
                    <span className="text-text-dim font-mono text-sm">
                      Recycled .cell name has no live cell.
                    </span>
                  )}
                </DataField>
                <DataField label="Created Transaction" layout="vertical" valueClassName="w-full">
                  <Link
                    href={`/tx/${detail.createdAtTx}`}
                    className="text-emphasis font-mono hover:underline"
                  >
                    <HexDisplay value={detail.createdAtTx} size="sm" />
                  </Link>
                </DataField>
                <DataField label="Created Block">
                  <Link
                    href={`/blocks/${detail.createdAtBlock}`}
                    className="text-emphasis font-mono hover:underline"
                  >
                    #{formatNumber(detail.createdAtBlock)}
                  </Link>
                </DataField>
              </DataGrid>
            </TerminalPanelContent>
          </TerminalPanel>

          <TerminalPanel>
            <TerminalPanelHeader indicator="active">Activities</TerminalPanelHeader>
            <TerminalPanelContent padding="none">
              <div className="p-4">
                {isActivitiesLoading ? (
                  <div className="text-text-dim py-2 text-sm">Loading activities...</div>
                ) : !itemActivities?.data?.length ? (
                  <div className="text-text-dim py-2 text-sm">No related activities found.</div>
                ) : (
                  <div className="space-y-2">
                    {itemActivities.data.map((activity) => (
                      <IdentityActivityCard
                        key={`${activity.blockNumber}-${activity.txIndex}-${activity.txHash}`}
                        txHash={activity.txHash}
                        blockNumber={activity.blockNumber}
                        txIndex={activity.txIndex}
                        timestamp={formatActivityTimestamp(activity.timestamp)}
                        actions={activity.actions}
                        normalizeAction={normalizeActivityAction}
                      />
                    ))}
                  </div>
                )}
              </div>
            </TerminalPanelContent>
            <TerminalPanelFooter>
              <CursorPagination
                total={itemActivities?.total ?? undefined}
                totalLabel="activities"
                pageSize={DEFAULT_PAGE_SIZE}
                page={activityCursorHistory.length + 1}
                hasMore={itemActivities?.hasMore ?? false}
                hasPrevious={activityCursorHistory.length > 0}
                onNext={() => goToNextActivityPage(itemActivities?.nextCursor)}
                onPrevious={goToPreviousActivityPage}
              />
            </TerminalPanelFooter>
          </TerminalPanel>
        </div>
      </main>
    </div>
  );
}
