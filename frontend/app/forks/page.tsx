'use client';

import { keepPreviousData, useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { Header } from '@/components/layout/header';
import { DetailLayout, DetailSection } from '@/components/ui/detail-layout';
import { ListTable } from '@/components/ui/list-table';
import { Chart } from '@/components/ui/chart';
import { CursorPagination } from '@/components/ui/cursor-pagination';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import { api, type ReorgEvent } from '@/lib/api';
import { DEFAULT_PAGE_SIZE } from '@/lib/pagination';

const eventLink = (event: ReorgEvent) => `/forks/${event.id}`;
const hash = (value: string) => <span className="break-all font-mono">{value}</span>;

export default function ForksPage() {
  const { cursor, hasPrevious, page, goToNext, goToPrevious } = useCursorPagination();
  const limit = DEFAULT_PAGE_SIZE;
  const overview = useQuery({
    queryKey: ['forks-overview', 30],
    queryFn: () => api.getForkOverview(30),
  });
  const events = useQuery({
    queryKey: ['forks', cursor, limit],
    queryFn: () => api.getForks({ cursor, limit }),
    placeholderData: keepPreviousData,
  });
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto min-w-0 px-4 py-8">
        <DetailLayout
          title="Forks & uncles"
          kind="index"
          identity="Uncles belong to canonical chain history. Branch switches are events observed by this node."
          facts={
            overview.data
              ? [
                  {
                    label: 'Uncles · 30 completed days',
                    value: overview.data.unclesInWindow.toLocaleString(),
                  },
                  {
                    label: 'Node switches · 30 days',
                    value: overview.data.nodeSwitchesInWindow.toLocaleString(),
                  },
                  {
                    label: 'Node switches · all time',
                    value: overview.data.nodeSwitchesAllTime.toLocaleString(),
                  },
                  { label: 'Through', value: overview.data.throughDate ?? 'No completed day' },
                ]
              : undefined
          }
        >
          <DetailSection title="Uncles and branch switches by day">
            {overview.isLoading && <p role="status">Loading fork history…</p>}
            {overview.error && (
              <p role="alert">Fork history unavailable: {(overview.error as Error).message}</p>
            )}
            {overview.data && (
              <>
                <p className="text-text-dim mb-4 text-xs">{overview.data.scope}</p>
                {overview.data.days.length ? (
                  <Chart
                    title="Chain uncles and this node's switches"
                    subtitle="Exact daily counts; hover or use arrow keys for a day"
                    data={overview.data.days.map((day) => ({
                      label: day.date,
                      values: { uncles: String(day.uncles), switches: String(day.nodeSwitches) },
                    }))}
                    series={[
                      { key: 'uncles', label: 'Uncles', color: '#54a8c7' },
                      { key: 'switches', label: 'Node switches', color: '#ad789f' },
                    ]}
                    unit="events"
                    kind="bars"
                  />
                ) : (
                  <p>No completed fork days are indexed.</p>
                )}
              </>
            )}
          </DetailSection>
          <DetailSection title="Observed branch switches">
            <p className="text-text-dim mb-4 text-xs">
              This node's persisted switches. An old branch can later re-enter the canonical chain.
            </p>
            {events.isLoading && <p role="status">Loading branch switches…</p>}
            {events.error && (
              <p role="alert">Branch switches unavailable: {(events.error as Error).message}</p>
            )}
            {events.data && (
              <>
                <ListTable
                  label="Observed branch switches"
                  rows={events.data.data}
                  rowKey={(event) => String(event.id)}
                  empty="No branch switches observed by this node. Chain uncles above can still exist."
                  columns={[
                    {
                      key: 'event',
                      label: 'Event',
                      minWidth: 175,
                      render: (event) => (
                        <Link href={eventLink(event)} className="text-emphasis hover:underline">
                          {new Date(event.detectedAt).toLocaleString()}
                        </Link>
                      ),
                    },
                    {
                      key: 'fork',
                      label: 'Fork point',
                      minWidth: 115,
                      render: (event) => (
                        <Link href={`/blocks/${event.forkPointNumber}`}>
                          #{event.forkPointNumber.toLocaleString()}
                        </Link>
                      ),
                    },
                    {
                      key: 'branch',
                      label: 'Old → adopted tip',
                      minWidth: 155,
                      render: (event) => (
                        <span>
                          #{event.oldTipNumber.toLocaleString()} → #
                          {event.newTipNumber.toLocaleString()}
                        </span>
                      ),
                    },
                    {
                      key: 'depth',
                      label: 'Depth',
                      minWidth: 70,
                      numeric: true,
                      render: (event) => event.depth.toLocaleString(),
                    },
                    {
                      key: 'fate',
                      label: 'Old block fate',
                      minWidth: 155,
                      render: (event) =>
                        event.preview?.fate.replaceAll('_', ' ') ??
                        event.previewStatus.replaceAll('_', ' '),
                    },
                    {
                      key: 'uncle',
                      label: 'Uncle in',
                      minWidth: 100,
                      render: (event) =>
                        event.preview?.uncleInBlock != null ? (
                          <Link href={`/blocks/${event.preview.uncleInBlock}`}>
                            #{event.preview.uncleInBlock}
                          </Link>
                        ) : (
                          '—'
                        ),
                    },
                  ]}
                  mobileRow={(event) => (
                    <div className="min-w-0 space-y-2 text-xs">
                      <div className="flex flex-wrap justify-between gap-2">
                        <Link href={eventLink(event)} className="text-emphasis">
                          {new Date(event.detectedAt).toLocaleString()}
                        </Link>
                        <span>
                          {event.depth} block{event.depth === 1 ? '' : 's'}
                        </span>
                      </div>
                      <div>
                        Fork #{event.forkPointNumber.toLocaleString()} · old #
                        {event.oldTipNumber.toLocaleString()} → adopted #
                        {event.newTipNumber.toLocaleString()}
                      </div>
                      <div>
                        Old block:{' '}
                        {event.preview?.fate.replaceAll('_', ' ') ??
                          event.previewStatus.replaceAll('_', ' ')}
                      </div>
                      {event.preview && (
                        <div>
                          Uncle inclusion:{' '}
                          {event.preview.uncleInBlock == null
                            ? 'none observed'
                            : `#${event.preview.uncleInBlock}`}{' '}
                          · later committed transactions: {event.preview.committedLaterTransactions}
                        </div>
                      )}
                      <div className="text-text-dim break-all">
                        Old tip {hash(event.oldTipHash)}
                      </div>
                    </div>
                  )}
                />
                <div className="mt-5">
                  <CursorPagination
                    total={events.data.total}
                    totalLabel="events"
                    pageSize={limit}
                    page={page}
                    hasMore={events.data.hasMore}
                    hasPrevious={hasPrevious}
                    onNext={() => goToNext(events.data!.nextCursor)}
                    onPrevious={goToPrevious}
                  />
                </div>
              </>
            )}
          </DetailSection>
        </DetailLayout>
      </main>
    </div>
  );
}
