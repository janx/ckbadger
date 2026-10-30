'use client';
import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { Header } from '@/components/layout/header';
import { DetailLayout, DetailSection } from '@/components/ui/detail-layout';
import { CellMicroscope } from '@/components/cell/cell-microscope';
import { CodeCellBoard } from '@/components/cell/code-cell-board';
import { CellTransactions } from '@/components/cell/cell-transactions';
import { CellBar } from '@/components/ui/cell-bar';
import { ExactCkb } from '@/components/activity-event-row';
import { parseCkbDecimal } from '@/lib/ckb-decimal';
import { api, isNotFoundApiError, type Cell } from '@/lib/api';
import { NotFoundPage } from '@/components/not-found-page';
import { useParams } from '@/src/navigation';
import { formatDurationMs, formatUtc8DateTime } from '@/lib/utils';
import { formatBlockNumber } from '@/lib/format-block';

function outpointParts(outpoint: string): { hash: string; index: number } | null {
  const match = /^(0x[0-9a-fA-F]{64})-(0|[1-9][0-9]*)$/.exec(outpoint);
  if (!match) return null;
  const index = Number(match[2]);
  return Number.isSafeInteger(index) && index <= 65535 ? { hash: match[1], index } : null;
}

function checkedCapacity(cell: Cell): { capacity: bigint; occupied: bigint } {
  if (
    !/^(0|[1-9][0-9]*)$/.test(cell.capacity) ||
    !/^(0|[1-9][0-9]*)$/.test(cell.commonKnowledgeSize)
  )
    throw new Error(
      `Cell ${cell.txHash}:${cell.outputIndex} has invalid capacity or occupied capacity`
    );
  const capacity = BigInt(cell.capacity);
  const occupied = BigInt(cell.commonKnowledgeSize);
  if (occupied > capacity || occupied === BigInt(0))
    throw new Error(
      `Cell ${cell.txHash}:${cell.outputIndex} occupied capacity exceeds capacity or is zero`
    );
  const breakdown = cell.commonKnowledgeSizeBreakdown;
  if (
    !breakdown ||
    breakdown.capacityFieldBytes +
      breakdown.lockScriptBytes +
      breakdown.typeScriptBytes +
      breakdown.dataBytes !==
      breakdown.totalBytes ||
    BigInt(breakdown.totalBytes) * BigInt(100000000) !== occupied
  )
    throw new Error(
      `Cell ${cell.txHash}:${cell.outputIndex} anatomy differs from indexed occupied capacity`
    );
  return { capacity, occupied };
}

function CellFacts({ cell }: { cell: Cell }) {
  let amounts: { capacity: bigint; occupied: bigint };
  try {
    amounts = checkedCapacity(cell);
  } catch (error) {
    return (
      <p role="alert" data-testid="cell-capacity-error" className="text-rouge text-xs">
        {String(error)}
      </p>
    );
  }
  const { capacity, occupied } = amounts;
  const breakdown = cell.commonKnowledgeSizeBreakdown!;
  return (
    <>
      <div className="border-base-border flex flex-wrap gap-x-10 gap-y-3 border-y py-4 text-xs">
        <div>
          <span className="text-text-dim">Capacity </span>
          <ExactCkb shannons={capacity} signed={false} tone="plain" />
        </div>
        <div>
          <span className="text-text-dim">Occupied </span>
          <ExactCkb shannons={occupied} signed={false} tone="plain" />
        </div>
        <div>
          <span className="text-text-dim">Free </span>
          <ExactCkb shannons={capacity - occupied} signed={false} tone="plain" />
        </div>
        <div>
          <span className="text-text-dim">Data </span>
          {cell.dataSize.toLocaleString()} bytes
        </div>
      </div>
      <CellBar
        subject={`cell ${cell.txHash}:${cell.outputIndex}`}
        anatomy={{
          lock: String(breakdown.lockScriptBytes),
          type: String(breakdown.typeScriptBytes),
          data: String(breakdown.dataBytes),
        }}
      />
    </>
  );
}

function CellIdentity({ cell }: { cell: Cell }) {
  return (
    <div className="grid gap-x-10 gap-y-2 text-xs sm:grid-cols-2">
      <p className="min-w-0 break-all">
        <span className="text-text-dim">Owner </span>
        {cell.address ? (
          <Link href={`/address/${cell.address}`} className="text-[#7aa7d6]">
            {cell.address}
          </Link>
        ) : (
          'Address unavailable'
        )}
      </p>
      <p className="min-w-0 break-all">
        <span className="text-text-dim">Lock </span>
        <Link
          href={`/script/${cell.lock?.codeHash}?hashType=${cell.lock?.hashType}`}
          className="text-[#7aa7d6]"
        >
          {cell.lock?.codeHash ?? cell.lockScriptHash}
        </Link>
      </p>
      {cell.type && (
        <p className="min-w-0 break-all">
          <span className="text-text-dim">Type </span>
          <Link
            href={`/script/${cell.type.codeHash}?hashType=${cell.type.hashType}`}
            className="text-[#b5c77a]"
          >
            {cell.type.codeHash}
          </Link>
        </p>
      )}
      <p>
        <span className="text-text-dim">Created </span>
        <Link href={`/blocks/${cell.createdAtBlock}`} className="text-text-bright">
          block {formatBlockNumber(cell.createdAtBlock)}
        </Link>
        {cell.createdTimestamp && (
          <>
            {' '}
            ·{' '}
            <time dateTime={cell.createdTimestamp}>
              {formatUtc8DateTime(cell.createdTimestamp)}
            </time>
          </>
        )}
      </p>
      {cell.status === 'dead' && (
        <p>
          <span className="text-text-dim">Spent </span>
          {cell.consumedAtBlock && (
            <Link href={`/blocks/${cell.consumedAtBlock}`} className="text-text-bright">
              block {formatBlockNumber(cell.consumedAtBlock)}
            </Link>
          )}
          {cell.spentTimestamp && (
            <>
              {' '}
              ·{' '}
              <time dateTime={cell.spentTimestamp}>{formatUtc8DateTime(cell.spentTimestamp)}</time>
            </>
          )}
          {cell.consumingInputIndex != null && <> · input #{cell.consumingInputIndex}</>}
          {cell.lifetimeMs != null && <> · {formatDurationMs(cell.lifetimeMs)} on chain</>}
        </p>
      )}
      {cell.status === 'live' && cell.liveAgeMs != null && (
        <p>
          <span className="text-text-dim">Age at indexed tip </span>
          {formatDurationMs(cell.liveAgeMs)}
          {cell.liveAgeAtBlock != null && <> · block {formatBlockNumber(cell.liveAgeAtBlock)}</>}
        </p>
      )}
    </div>
  );
}

export default function CellDetailPage() {
  const { outpoint } = useParams();
  const parts = typeof outpoint === 'string' ? outpointParts(outpoint) : null;
  const query = useQuery({
    queryKey: ['cell', parts?.hash, parts?.index],
    queryFn: () => api.getCell(parts!.hash, parts!.index),
    enabled: parts !== null,
  });
  if (parts && !query.data && isNotFoundApiError(query.error)) {
    return (
      <NotFoundPage
        entity={{
          kind: 'cell',
          identifier: `${parts.hash}-${parts.index}`,
          checkedEndpoint: `/cells/${parts.hash}/${parts.index}`,
        }}
      />
    );
  }
  const body = !parts ? (
    <DetailLayout title="Invalid cell outpoint">
      <p role="alert" className="text-rouge break-all text-xs">
        {String(outpoint)}
      </p>
    </DetailLayout>
  ) : query.isPending ? (
    <DetailLayout title="Cell">
      <p className="text-text-dim text-xs">Loading cell evidence…</p>
    </DetailLayout>
  ) : query.error && !query.data ? (
    <DetailLayout title="Cell data unavailable">
      <p role="alert" className="text-rouge break-all text-xs">
        {query.error.message}
      </p>
    </DetailLayout>
  ) : !query.data ? (
    <DetailLayout title="Cell data unavailable">
      <p role="alert" className="text-rouge text-xs">
        Cell response was empty.
      </p>
    </DetailLayout>
  ) : (
    <DetailLayout
      title="Cell"
      identity={`${query.data.txHash}:${query.data.outputIndex}`}
      status={
        <span
          className={
            query.data.status === 'live' ? 'text-xs text-[#2edba3]' : 'text-xs text-[#f08aa8]'
          }
        >
          {query.data.status === 'live' ? 'Live' : 'Spent'}
        </span>
      }
    >
      {query.error && (
        <p role="alert">
          Unable to refresh cell: {query.error.message}. Showing previously loaded data.
        </p>
      )}
      <CellFacts cell={query.data} />
      <CellIdentity cell={query.data} />
      {query.data.daoInfo && (
        <DetailSection title="Nervos DAO">
          <p className="text-text-dim text-xs">
            {query.data.daoInfo.daoStatus} · deposit block{' '}
            {formatBlockNumber(query.data.daoInfo.depositBlockNumber)}
            {query.data.daoInfo.compensationCkb != null ? (
              <>
                {' '}
                · compensation{' '}
                <ExactCkb
                  shannons={parseCkbDecimal(query.data.daoInfo.compensationCkb)}
                  signed={false}
                  tone="plain"
                />
              </>
            ) : query.data.daoInfo.compensation != null ? (
              (() => {
                throw new Error(
                  `DAO cell ${query.data.txHash}:${query.data.outputIndex} has compensation shannons without compensationCkb`
                );
              })()
            ) : null}
          </p>
        </DetailSection>
      )}
      <CellMicroscope cell={query.data} />
      {query.data.elf && <CodeCellBoard cell={query.data} />}
      {query.data.isDepGroup && (
        <DetailSection title="Dep group">
          <ul className="space-y-2 text-xs">
            {query.data.depGroupItems?.map((item) => (
              <li key={`${item.txHash}:${item.outputIndex}`}>
                <Link href={`/cell/${item.txHash}-${item.outputIndex}`} className="text-[#7aa7d6]">
                  {item.txHash}:{item.outputIndex}
                </Link>
              </li>
            ))}
          </ul>
        </DetailSection>
      )}
      <CellTransactions cell={query.data} />
    </DetailLayout>
  );
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">{body}</main>
    </div>
  );
}
