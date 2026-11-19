'use client';
import type { ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { IdCopyRow } from '@/components/ui/head-id';
import { isAwaited } from '@/lib/query-state';
import Link from '@/components/ui/link';
import { DetailLayout, DetailSection } from '@/components/ui/detail-layout';
import { DetailSkeleton } from '@/components/ui/detail-skeleton';
import { CellMicroscope } from '@/components/cell/cell-microscope';
import { CodeCellBoard } from '@/components/cell/code-cell-board';
import { CellScriptGroups, CellTransactions } from '@/components/cell/cell-transactions';
import { CellInventoryLine } from '@/components/cell/inventory-line';
import { kindOf } from '@/components/transaction/transaction-cell-tiles';
import { CkbAmount } from '@/components/ui/ckb-amount';
import { parseShannons } from '@/lib/ckb-decimal';
import { api, isNotFoundApiError, type Cell } from '@/lib/api';
import { isBadRequestApiError } from '@/lib/api-errors';
import { NotFoundPage } from '@/components/not-found-page';
import { ReadFailure } from '@/components/read-failure';
import { useParams } from '@/src/navigation';
import { formatDurationMs } from '@/lib/utils';
import { shortenOutpoint } from '@/lib/shorten';
import { DATA_KEY } from '@/lib/design-tokens';
import { formatBlockNumber } from '@/lib/format-block';
import { formatCount } from '@/lib/format-count';
import { OutpointIndex } from '@/components/ui/outpoint-index';
import { BlockLink } from '@/components/ui/block-link';
import { titleHash } from '@/lib/use-document-title';
import { cellPageSeal } from '@/lib/cell-seal';
import { ShortId } from '@/components/ui/short-id';
import { TitleId } from '@/components/ui/title-id';
import { Utc8Age, Utc8DateTime } from '@/components/ui/utc8-date';
import type { StripFact } from '@/components/ui/status-strip';
import { cellLockFacts, lockWords, type LockWords } from '@/components/ui/lock-kind';

/** What `outpointParts` accepts, as the not-an-id page states it. */
const OUTPOINT_RULE =
  'an out point is a transaction hash and an output index: 0x and 64 hex digits, a dash, 0 to 65535.';

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

/** A dep group's members: the API sends them with every dep group, so none is a broken read. */
function depGroupItems(cell: Cell): NonNullable<Cell['depGroupItems']> {
  if (!cell.depGroupItems)
    throw new Error(`Cell ${cell.txHash}:${cell.outputIndex} is a dep group without its members`);
  return cell.depGroupItems;
}

/**
 * Where a DAO cell's compensation stands and the block the API read it at (F-9): still accruing
 * (the tip), fixed at the withdrawal request's block, or claimed at the completion's. The API sends
 * the state with every DAO cell, so any other value is a broken read.
 */
function DaoCompensationAt({ cell, dao }: { cell: Cell; dao: NonNullable<Cell['daoInfo']> }) {
  const block = <BlockLink look="inline" number={dao.compensationAtBlock} />;
  switch (dao.compensationState) {
    case 'accruing':
      return <>, at block {block}</>;
    case 'frozen':
      return <>, fixed at the withdrawal request {block}</>;
    case 'claimed':
      return <>, claimed at block {block}</>;
    default:
      throw new Error(
        `DAO cell ${cell.txHash}:${cell.outputIndex} has compensation state ${String(dao.compensationState)}`
      );
  }
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
  byHashType = null,
  owner,
}: {
  label: string;
  script: Cell['lock'];
  name: string | null;
  /** "by <hash type>" after a lock the one decision writes by its code hash (`LockWords`). */
  byHashType?: string | null;
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
  // a fact tile's ids at the row size (U-09)
  const text = name ?? <ShortId value={script.codeHash} kind="hash" size="row" />;
  return (
    <Fact
      label={label}
      sub={
        script.args !== '0x' || owner ? (
          <>
            {script.args !== '0x' && (
              <span title={script.args}>
                args <ShortId value={script.args} kind="hash" size="row" />
              </span>
            )}
            {owner && (
              <Link
                look="inline"
                href={`/address/${owner}`}
                title={owner}
                className="block [overflow-wrap:anywhere]"
              >
                <ShortId value={owner} kind="address" size="row" />
              </Link>
            )}
          </>
        ) : undefined
      }
    >
      {href ? (
        <Link look="inline" href={href} title={`${script.hashType} ${script.codeHash}`}>
          {text}
        </Link>
      ) : (
        <span title={script.codeHash}>{text}</span>
      )}
      {byHashType && ` by ${byHashType}`}
    </Fact>
  );
}

function CellFacts({
  cell,
  lock,
  typeName,
}: {
  cell: Cell;
  /** The lock's words, the one decision's in a fact (`lockWords`, as a column's: never empty). */
  lock: LockWords | null;
  typeName: string | null;
}) {
  let amounts: { capacity: bigint; occupied: bigint };
  try {
    amounts = checkedCapacity(cell);
  } catch (error) {
    return (
      <p
        role="alert"
        data-testid="cell-capacity-error"
        className="text-rouge text-small max-w-measure"
      >
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
      {/* The capacity, the time and the block it was created in are the status strip's (lane D2):
          these are the cell's other facts, none deleted. */}
      <Fact label="Occupied">
        <span>{breakdown.totalBytes.toLocaleString('en-US')} B</span>{' '}
        <span className="text-text-dim text-small">
          ·{' '}
          {capacity === occupied ? (
            '0'
          ) : (
            <CkbAmount form="exact" shannons={capacity - occupied} signed={false} tone="plain" />
          )}{' '}
          free
        </span>
      </Fact>
      <ScriptFact
        label="Lock"
        script={cell.lock}
        name={lock && lock.form !== 'not-indexed' ? lock.name : null}
        byHashType={lock?.form === 'lock' ? lock.byHashType : null}
        owner={cell.address}
      />
      <ScriptFact label="Type" script={cell.type} name={typeName} />
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
        <span>{cell.dataSize.toLocaleString('en-US')} B</span>
      </Fact>
      {/* the epoch it was created in (the strip states its block and time) */}
      <Fact label="Epoch">
        {cell.createdEpoch ? (
          `${cell.createdEpoch.number.toLocaleString('en-US')} · ${cell.createdEpoch.index}/${cell.createdEpoch.length}`
        ) : (
          <span role="alert" className="text-rouge">
            Cell {cell.txHash}:{cell.outputIndex} has no createdEpoch
          </span>
        )}
      </Fact>
      {cell.status === 'dead' ? (
        <Fact
          label="Spent"
          sub={
            <>
              {cell.spentTimestamp && (
                <Utc8DateTime timestamp={cell.spentTimestamp} dateTime={cell.spentTimestamp} />
              )}
              {cell.consumingInputIndex != null && <> · input #{cell.consumingInputIndex}</>}
              {cell.lifetimeMs != null && <> · {formatDurationMs(cell.lifetimeMs)} on chain</>}
            </>
          }
        >
          {cell.consumedAtBlock != null ? (
            <Link look="key" href={`/blocks/${cell.consumedAtBlock}`}>
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
                  as of <BlockLink look="inline" number={cell.liveAgeAtBlock} />
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

/**
 * The cell's status strip (lane D2, PA-13; gate D0′'s strip with the cell's own four facts): its
 * state, live (the data's celadon dot) or spent (the dead ink's), the time it was created on the
 * site's clock with its age, the block that created it and its capacity, exact. A time the API did
 * not send is not passed.
 */
function cellStripFacts(cell: Cell): StripFact[] {
  return [
    {
      label: 'status',
      value: cell.status === 'live' ? 'live' : 'spent',
      dot: cell.status === 'live' ? DATA_KEY : 'text-dim',
    },
    ...(cell.createdTimestamp
      ? [
          {
            label: 'created',
            value: <Utc8DateTime timestamp={cell.createdTimestamp} />,
            note: <Utc8Age timestamp={cell.createdTimestamp} />,
          },
        ]
      : []),
    {
      label: 'block',
      value: (
        <Link look="inline" href={`/blocks/${cell.createdAtBlock}`}>
          {formatBlockNumber(cell.createdAtBlock)}
        </Link>
      ),
    },
    {
      label: 'capacity',
      value: (
        <CkbAmount
          form="exact"
          shannons={parseShannons(
            cell.capacity,
            `cell ${cell.txHash}:${cell.outputIndex} capacity`
          )}
          signed={false}
          tone="inherit"
        />
      ),
    },
  ];
}

function CellBody({ cell, refreshError }: { cell: Cell; refreshError: Error | null }) {
  const names = useScriptNames(cell);
  const outpoint = `${cell.txHash}:${cell.outputIndex}`;
  // The lock's words, the one decision's (components/ui/lock-kind.tsx; lane P1 follow-up, the
  // lock-words gate): its script and the registry's name for its code hash, the address the API
  // spells checked against the script. The cell's facts name the default lock too (a column's
  // form); a lock on the default lock's code hash by data is not the default lock, and the
  // registry's name, which is the default lock's, is not written for it.
  const lock = cell.lock
    ? lockWords(
        cellLockFacts(
          { lock: cell.lock, name: names.lock, address: cell.address ?? null },
          `cell ${outpoint}`
        ),
        'column'
      )
    : null;
  const lockLabel = lock && lock.form !== 'not-indexed' ? lock.name : null;
  // The seals by kind (C3): the cell's own stone, classed by the transaction page's one
  // classifier from the class fields the API serves for it; spent, in the dead ink.
  const seal = cellPageSeal(cell, kindOf);
  return (
    <>
      <DetailLayout
        // Consistency round C13 + lane U (U3): the page presents a cell, named by its short out
        // point; the kind line before it says "Cell" (seals by kind, C10), the full out point and
        // its copy are the id line, and the transaction it is an output of is said under the plate
        // (no "Transaction … / output N" breadcrumb). The h1 is the out point alone, one unit,
        // at the head size where its measured line holds it and at the row size otherwise (U-09,
        // RESPONSIVE_LAYOUT principle 4; TitleId). Below md it has the full width under the stone at
        // 26 px, 15.08 px a character with the h1's tracking: the head out point (23 to 25
        // characters for an index up to 999) is 346.8 to 377.0 px, so an index under 10 takes the
        // head form in the 358 px line at 390 and every out point takes the row form (19 to 21
        // characters, 286.5 to 316.7 px) in the 328 px line at 360; from md, beside the stone, the
        // head form (400.2 to 435 px at 30 px) fits the plate.
        title={<TitleId form={(size) => shortenOutpoint(cell.txHash, cell.outputIndex, size)} />}
        documentTitle={[`Cell ${titleHash(cell.txHash)}:${cell.outputIndex}`]}
        // The cell's own page seal: its transaction's rim and border, its index in rods, its
        // owner's stone, a bead for each digit of its capacity, its type's class and glyph.
        seal={{ ...seal, kind: 'Cell' }}
        // The status strip under the id line (lane D2, PA-13): its state, when it was created, the
        // block that created it and its capacity; its other facts stay in "Cell facts" below.
        strip={{ label: 'Cell status', facts: cellStripFacts(cell) }}
        // The id line: the full out point and its copy, in the plate's quiet voice (C10 round 2:
        // it was text-body in the bright ink, competing with the h1 above it), its colon in the
        // same ink; it wraps anywhere, as the line does. The id line's one form (IdCopyRow, lane
        // c2-finish rulings round 2): its copy 8 px after it at its first line, its answer under
        // the whole out point; not a HeadId, whose kinds are a hash, an address and a did:ckb.
        identity={
          <IdCopyRow value={outpoint} copy="copy out point">
            <span className="min-w-0">{outpoint}</span>
          </IdCopyRow>
        }
        // Under the stone and the plate, from the page's left edge (C10): one sentence of what the
        // cell is (lane U, U6: the inventory item it is; lane P1: for every kind, by its scripts —
        // the code it holds, which the kind line said, among them). Said once (lane P1): its
        // state, its block and its capacity are the strip's, its transaction the Transactions
        // section's link, and its out point is written whole on the id line and short in the h1
        // (the origin line under them wrote the transaction's hash a third time); what an out
        // point is, in general, is the developer tier's (lane D3).
        lede={
          <CellInventoryLine
            cell={cell}
            typeName={
              cell.type?.codeHash === TYPE_ID_CODE_HASH || names.lookup.isSuccess
                ? names.type
                : undefined
            }
          />
        }
        // The developer tier (lane D3, U-17): what an out point is, the cell's bytes, and the
        // script groups of its transactions, in the transaction page's rows.
        tier2={
          <>
            {/* a sentence that explains: the reading ink at the body step (R3, R4) */}
            <p className="text-text text-body max-w-measure">
              The out point is the cell&apos;s only name: transaction hash and output index. No
              other cell has it, and this one{' '}
              {cell.status === 'dead' && cell.consumedAtBlock != null ? (
                <>
                  existed from block <BlockLink look="inline" number={cell.createdAtBlock} /> until
                  block <BlockLink look="inline" number={cell.consumedAtBlock} /> spent it.
                </>
              ) : (
                <>
                  exists from block <BlockLink look="inline" number={cell.createdAtBlock} /> until a
                  transaction spends it.
                </>
              )}
            </p>
            <CellMicroscope
              key={outpoint}
              cell={cell}
              lockLabel={lockLabel}
              typeName={names.type}
            />
            <CellScriptGroups cell={cell} />
          </>
        }
      >
        {refreshError && (
          <p className="max-w-measure" role="alert">
            Unable to refresh cell: {refreshError.message}. Showing previously loaded data.
          </p>
        )}
        <CellFacts cell={cell} lock={lock} typeName={names.type} />
        {cell.daoInfo && (
          // The command runs on the status the API gives (deposited, withdrawing, withdrawn); the
          // heading says it, so the line under it does not say it again.
          <DetailSection title="Nervos DAO" args={cell.daoInfo.daoStatus}>
            {/* F-9: the API sends every DAO cell's exact compensation (shannons) with its state
                and block; an answer without them is a broken read and fails here, by name. */}
            <p className="text-text-dim text-small max-w-measure">
              deposit block <BlockLink look="inline" number={cell.daoInfo.depositBlockNumber} /> ·{' '}
              {cell.daoInfo.compensationState === 'accruing' && 'unclaimed '}compensation{' '}
              <CkbAmount
                form="exact"
                shannons={parseShannons(
                  cell.daoInfo.compensation,
                  `DAO cell ${cell.txHash}:${cell.outputIndex} compensation`
                )}
                signed={false}
                tone="plain"
              />
              <DaoCompensationAt cell={cell} dao={cell.daoInfo} />
            </p>
          </DetailSection>
        )}
        {cell.elf && <CodeCellBoard cell={cell} />}
        {cell.isDepGroup && (
          <DetailSection title="Dep group" args={formatCount(depGroupItems(cell).length, 'item')}>
            <ul className="text-small space-y-2">
              {depGroupItems(cell).map((item) => (
                <li key={`${item.txHash}:${item.outputIndex}`}>
                  <Link href={`/cell/${item.txHash}-${item.outputIndex}`} look="inline">
                    {item.txHash}
                    <OutpointIndex index={item.outputIndex} />
                  </Link>
                </li>
              ))}
            </ul>
          </DetailSection>
        )}
        <CellTransactions cell={cell} lockLabel={lockLabel} typeName={names.type} />
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
  if (!parts) return <NotFoundPage notAnId={{ kind: 'cell', reason: OUTPOINT_RULE }} />;
  if (!query.data && isBadRequestApiError(query.error))
    return <NotFoundPage notAnId={{ kind: 'cell', reason: query.error.apiMessage }} />;
  const body = isAwaited(query) ? (
    <DetailSkeleton />
  ) : query.error && !query.data ? (
    <ReadFailure what="cell" error={query.error} />
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
