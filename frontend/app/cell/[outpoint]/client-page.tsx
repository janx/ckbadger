'use client';
import { useState, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { DetailLayout, DetailSection } from '@/components/ui/detail-layout';
import { DetailSkeleton } from '@/components/ui/detail-skeleton';
import { CellMicroscope } from '@/components/cell/cell-microscope';
import { CodeCellBoard } from '@/components/cell/code-cell-board';
import { CellTransactions } from '@/components/cell/cell-transactions';
import { CellInventoryLine } from '@/components/cell/inventory-line';
import { ExactCkb } from '@/components/activity-event-row';
import { parseCkbDecimal } from '@/lib/ckb-decimal';
import { api, isNotFoundApiError, type Cell } from '@/lib/api';
import { NotFoundPage } from '@/components/not-found-page';
import { useParams } from '@/src/navigation';
import { formatDurationMs, formatUtc8DateTime, truncateHash } from '@/lib/utils';
import { BOARD_INKS, CELL_COLORS, DATA_COLOR, EXPIRED_COLOR, inkStyle } from '@/lib/design-tokens';
import { formatBlockNumber } from '@/lib/format-block';
import { OutpointIndex } from '@/components/ui/outpoint-index';
import { BlockLink } from '@/components/ui/block-link';
import { titleHash } from '@/lib/use-document-title';

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

const TYPE_ID_CODE_HASH = '0x00000000000000000000000000000000000000000000000000545950455f4944';

/** Registry names of the cell's lock and type, the way the transaction page names its tiles. */
function useScriptNames(cell: Cell | undefined) {
  const codeHashes = cell
    ? [cell.lock?.codeHash, cell.type?.codeHash].filter(
        (hash): hash is string => hash !== undefined && hash !== TYPE_ID_CODE_HASH
      )
    : [];
  const lookup = useQuery({
    queryKey: ['scriptLookup', codeHashes, cell?.txHash],
    queryFn: () => api.lookupScripts(codeHashes, cell!.txHash),
    enabled: codeHashes.length > 0,
    staleTime: Infinity,
  });
  const name = (codeHash: string | undefined): string | null => {
    if (!codeHash) return null;
    if (codeHash === TYPE_ID_CODE_HASH) return 'Type ID';
    const found = lookup.data?.[codeHash]?.name;
    return found && found.toLowerCase() !== 'unknown' ? found : null;
  };
  return { lock: name(cell?.lock?.codeHash), type: name(cell?.type?.codeHash), lookup };
}

function short(hash: string, head = 8, tail = 6): string {
  return hash.length <= head + tail + 3 ? hash : `${hash.slice(0, 2 + head)}…${hash.slice(-tail)}`;
}

function Fact({ label, children, sub }: { label: string; children: ReactNode; sub?: ReactNode }) {
  return (
    <div className="min-w-0">
      <div className="text-text-dim">{label}</div>
      <div className="text-body mt-0.5">{children}</div>
      {sub && <div className="text-text-dim mt-0.5">{sub}</div>}
    </div>
  );
}

function ScriptFact({
  label,
  script,
  name,
  color,
  owner,
}: {
  label: string;
  script: Cell['lock'];
  name: string | null;
  color: string;
  /** Lane L: the lock's address, the cell's owner, linked under the args (the Lock fact only). */
  owner?: string;
}) {
  if (!script)
    return (
      <Fact label={label}>
        <span className="text-text-dim">none</span>
      </Fact>
    );
  const href =
    script.codeHash === TYPE_ID_CODE_HASH
      ? undefined
      : `/script/${script.codeHash}?hashType=${script.hashType}`;
  const text = name ?? short(script.codeHash);
  return (
    <Fact
      label={label}
      sub={
        script.args !== '0x' || owner ? (
          <>
            {script.args !== '0x' && (
              <span title={script.args}>args {short(script.args, 4, 4)}</span>
            )}
            {owner && (
              <Link
                href={`/address/${owner}`}
                title={owner}
                className="hover:text-jade block [overflow-wrap:anywhere]"
              >
                {truncateHash(owner, 10, 6)}
              </Link>
            )}
          </>
        ) : undefined
      }
    >
      {href ? (
        <Link href={href} title={`${script.hashType} ${script.codeHash}`} style={{ color }}>
          {text}
        </Link>
      ) : (
        <span style={{ color }} title={script.codeHash}>
          {text}
        </span>
      )}
    </Fact>
  );
}

function CellFacts({
  cell,
  lockName,
  typeName,
}: {
  cell: Cell;
  lockName: string | null;
  typeName: string | null;
}) {
  let amounts: { capacity: bigint; occupied: bigint };
  try {
    amounts = checkedCapacity(cell);
  } catch (error) {
    return (
      <p role="alert" data-testid="cell-capacity-error" className="text-rouge text-small">
        {String(error)}
      </p>
    );
  }
  const { capacity, occupied } = amounts;
  const breakdown = cell.commonKnowledgeSizeBreakdown!;
  return (
    <section
      aria-label="Cell facts"
      className="border-base-border text-small flex flex-wrap gap-x-10 gap-y-3.5 border-y py-3.5"
    >
      <Fact label="Capacity">
        <ExactCkb shannons={capacity} signed={false} tone="plain" />
      </Fact>
      <Fact label="Occupied">
        <span style={{ color: DATA_COLOR }}>{breakdown.totalBytes.toLocaleString('en-US')} B</span>{' '}
        <span className="text-text-dim text-small">
          ·{' '}
          {capacity === occupied ? (
            '0'
          ) : (
            <ExactCkb shannons={capacity - occupied} signed={false} tone="plain" />
          )}{' '}
          free
        </span>
      </Fact>
      <ScriptFact
        label="Lock"
        script={cell.lock}
        name={lockName}
        color={CELL_COLORS.lock}
        owner={cell.address}
      />
      <ScriptFact label="Type" script={cell.type} name={typeName} color={CELL_COLORS.type} />
      <Fact
        label="Data"
        sub={
          cell.elf?.status === 'parsed'
            ? 'RISC-V ELF'
            : cell.elf
              ? 'ELF, does not parse'
              : undefined
        }
      >
        <span style={{ color: CELL_COLORS.data }}>{cell.dataSize.toLocaleString('en-US')} B</span>
      </Fact>
      <Fact
        label="Created"
        sub={
          <>
            {cell.createdTimestamp && (
              <time dateTime={cell.createdTimestamp}>
                {formatUtc8DateTime(cell.createdTimestamp)}
              </time>
            )}
            {cell.createdEpoch ? (
              <span className="block">
                {`epoch ${cell.createdEpoch.number.toLocaleString('en-US')} · ${cell.createdEpoch.index}/${cell.createdEpoch.length}`}
              </span>
            ) : (
              <span role="alert" className="text-rouge block">
                Cell {cell.txHash}:{cell.outputIndex} has no createdEpoch
              </span>
            )}
          </>
        }
      >
        <Link href={`/blocks/${cell.createdAtBlock}`} className="text-text-bright">
          {formatBlockNumber(cell.createdAtBlock)}
        </Link>
      </Fact>
      {cell.status === 'dead' ? (
        <Fact
          label="Spent"
          sub={
            <>
              {cell.spentTimestamp && (
                <time dateTime={cell.spentTimestamp}>
                  {formatUtc8DateTime(cell.spentTimestamp)}
                </time>
              )}
              {cell.consumingInputIndex != null && <> · input #{cell.consumingInputIndex}</>}
              {cell.lifetimeMs != null && <> · {formatDurationMs(cell.lifetimeMs)} on chain</>}
            </>
          }
        >
          {cell.consumedAtBlock != null ? (
            <Link href={`/blocks/${cell.consumedAtBlock}`} className="text-text-bright">
              {formatBlockNumber(cell.consumedAtBlock)}
            </Link>
          ) : (
            <span className="text-text-dim">block unavailable</span>
          )}
        </Fact>
      ) : (
        cell.liveAgeMs != null && (
          <Fact
            label="Unspent for"
            sub={
              cell.liveAgeAtBlock != null && (
                <>
                  as of <BlockLink number={cell.liveAgeAtBlock} />
                </>
              )
            }
          >
            <span className="text-text-bright">{formatDurationMs(cell.liveAgeMs)}</span>
          </Fact>
        )
      )}
    </section>
  );
}

function CopyOutpoint({ value }: { value: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      aria-label="Copy out point"
      onClick={async () => {
        await navigator.clipboard.writeText(value);
        setCopied(true);
      }}
      className="text-text-dim hover:text-text-bright text-meta border-base-border h-[26px] rounded border px-2"
    >
      {copied ? 'copied' : 'copy'}
    </button>
  );
}

/** "code cell of JoyID, v5": the scripts this cell is code for, and its Type ID version. */
function CodeCellOf({ cell }: { cell: Cell }) {
  if (!cell.codeCellOf?.length) return null;
  const version = cell.codeCell?.typeIdVersions.find(
    (item) => item.codeCellTxHash === cell.txHash && item.codeCellOutputIndex === cell.outputIndex
  );
  return (
    <span data-testid="code-cell-of" className="text-text-dim text-small">
      · code cell of{' '}
      {cell.codeCellOf.map((script, i) => (
        <span key={`${script.codeHash}:${script.hashType}`}>
          {i > 0 && ', '}
          <Link
            href={`/script/${script.codeHash}?hashType=${script.hashType}`}
            className="text-text-bright"
          >
            {script.name}
          </Link>
        </span>
      ))}
      {version && `, v${version.ordinal}`}
    </span>
  );
}

/**
 * Lane U (U3): the transaction and the output index as one line under the full out point —
 * "output 0 of transaction 0x0361eb40…b6518a · live since #14,965,803", the hash linking to the
 * transaction page and the block to its page.
 */
function CellOrigin({ cell }: { cell: Cell }) {
  return (
    <p data-testid="cell-origin">
      output {cell.outputIndex} of transaction{' '}
      <Link href={`/tx/${cell.txHash}`} className="text-text hover:text-text-bright">
        {short(cell.txHash)}
      </Link>{' '}
      ·{' '}
      {cell.status === 'dead' ? (
        cell.consumedAtBlock != null ? (
          <span className="whitespace-nowrap">
            spent at{' '}
            <Link
              href={`/blocks/${cell.consumedAtBlock}`}
              className="text-text hover:text-text-bright"
            >
              {formatBlockNumber(cell.consumedAtBlock)}
            </Link>
          </span>
        ) : (
          'spent, block unavailable'
        )
      ) : (
        <span className="whitespace-nowrap">
          live since{' '}
          <Link
            href={`/blocks/${cell.createdAtBlock}`}
            className="text-text hover:text-text-bright"
          >
            {formatBlockNumber(cell.createdAtBlock)}
          </Link>
        </span>
      )}
    </p>
  );
}

function CellBody({ cell, refreshError }: { cell: Cell; refreshError: Error | null }) {
  const names = useScriptNames(cell);
  const outpoint = `${cell.txHash}:${cell.outputIndex}`;
  return (
    <>
      <DetailLayout
        // Consistency round C13 + lane U (U3): the page presents a cell — "Cell" in dim before
        // its short out point; the full out point, its copy and the transaction it is an output
        // of are the identity lines beneath (no "Transaction … / output N" breadcrumb).
        title={
          <>
            {/* An inline-block, so a phone wraps "Cell" away before it breaks the out point. */}
            <span className="text-text-dim">Cell</span>{' '}
            <span className="inline-block">
              {short(cell.txHash)}:{cell.outputIndex}
            </span>
          </>
        }
        documentTitle={[`Cell ${titleHash(cell.txHash)}:${cell.outputIndex}`]}
        // The out point's 32-byte part is its transaction's hash (the index is not a hash): a cell
        // carries the seal of the transaction that made it, the seal on that transaction's page.
        seal={{ hash: cell.txHash, scale: 'object' }}
        status={
          <span className="flex flex-wrap items-center gap-3">
            <span
              className="text-small inline-flex items-center gap-1.5"
              style={{ color: cell.status === 'live' ? DATA_COLOR : EXPIRED_COLOR }}
            >
              <span
                aria-hidden="true"
                className="inline-block h-[7px] w-[7px] rounded-full"
                style={{ backgroundColor: cell.status === 'live' ? DATA_COLOR : EXPIRED_COLOR }}
              />
              {cell.status === 'live' ? 'live' : 'spent'}
            </span>
            <CodeCellOf cell={cell} />
          </span>
        }
        identity={
          <div className="space-y-1.5">
            {/* Lane U (U6): the inventory item this cell is, first under the h1. */}
            <CellInventoryLine cell={cell} />
            <div className="flex flex-wrap items-center gap-2.5">
              <span className="text-text-bright text-body [overflow-wrap:anywhere]">
                {cell.txHash}
                <span className="text-text-dim">:</span>
                {cell.outputIndex}
              </span>
              <CopyOutpoint value={outpoint} />
            </div>
            <CellOrigin cell={cell} />
            <p>
              The out point is the cell&apos;s only name: transaction hash and output index. No
              other cell has it, and this one{' '}
              {cell.status === 'dead' && cell.consumedAtBlock != null ? (
                <>
                  existed from block <BlockLink number={cell.createdAtBlock} /> until block{' '}
                  <BlockLink number={cell.consumedAtBlock} /> spent it.
                </>
              ) : (
                <>
                  exists from block <BlockLink number={cell.createdAtBlock} /> until a transaction
                  spends it.
                </>
              )}
            </p>
          </div>
        }
      >
        {refreshError && (
          <p role="alert">
            Unable to refresh cell: {refreshError.message}. Showing previously loaded data.
          </p>
        )}
        <CellFacts cell={cell} lockName={names.lock} typeName={names.type} />
        {cell.daoInfo && (
          <DetailSection title="Nervos DAO">
            <p className="text-text-dim text-small">
              {cell.daoInfo.daoStatus} · deposit block{' '}
              <BlockLink number={cell.daoInfo.depositBlockNumber} />
              {cell.daoInfo.compensationCkb != null ? (
                <>
                  {' '}
                  · compensation{' '}
                  <ExactCkb
                    shannons={parseCkbDecimal(cell.daoInfo.compensationCkb)}
                    signed={false}
                    tone="plain"
                  />
                </>
              ) : cell.daoInfo.compensation != null ? (
                (() => {
                  throw new Error(
                    `DAO cell ${cell.txHash}:${cell.outputIndex} has compensation shannons without compensationCkb`
                  );
                })()
              ) : null}
            </p>
          </DetailSection>
        )}
        {cell.elf && <CodeCellBoard cell={cell} />}
        <CellMicroscope key={outpoint} cell={cell} lockName={names.lock} typeName={names.type} />
        {cell.isDepGroup && (
          <DetailSection title="Dep group">
            <ul className="text-small space-y-2">
              {cell.depGroupItems?.map((item) => (
                <li key={`${item.txHash}:${item.outputIndex}`}>
                  <Link
                    href={`/cell/${item.txHash}-${item.outputIndex}`}
                    className="text-[color:var(--ink)]"
                    style={inkStyle(BOARD_INKS.cellLink)}
                  >
                    {item.txHash}
                    <OutpointIndex index={item.outputIndex} />
                  </Link>
                </li>
              ))}
            </ul>
          </DetailSection>
        )}
        <CellTransactions cell={cell} lockName={names.lock} typeName={names.type} />
      </DetailLayout>
    </>
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
      <p role="alert" className="text-rouge text-small break-all">
        {String(outpoint)}
      </p>
    </DetailLayout>
  ) : query.isPending ? (
    <DetailSkeleton />
  ) : query.error && !query.data ? (
    <DetailLayout title="Cell data unavailable">
      <p role="alert" className="text-rouge text-small break-all">
        {query.error.message}
      </p>
    </DetailLayout>
  ) : !query.data ? (
    <DetailLayout title="Cell data unavailable">
      <p role="alert" className="text-rouge text-small">
        Cell response was empty.
      </p>
    </DetailLayout>
  ) : (
    <CellBody cell={query.data} refreshError={query.error} />
  );
  return (
    <div className="min-h-screen">
      <main className="container mx-auto py-8">{body}</main>
    </div>
  );
}
