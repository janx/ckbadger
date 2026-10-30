'use client';

import type { ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { TxStructure, type TxContinuity } from '@/components/transaction/tx-structure';
import { cellTiles } from '@/components/transaction/transaction-cell-tiles';
import { api, type TransactionDetail } from '@/lib/api';
import { formatBlockNumber } from '@/lib/format-block';
import { formatUtc8DateTime } from '@/lib/utils';
import { stageEpoch } from '@/lib/fiber-format';

export interface FiberStepBox {
  head: ReactNode;
  lines: ReactNode[];
}

interface Props {
  n: number;
  title: string;
  txHash: string;
  blockNumber: number;
  timestamp: string;
  description: ReactNode;
  /** The stage block's epoch from the channel evidence (`number+index/length`), when loaded. */
  epoch?: string;
  /** The channel cell this step spends (funding or commitment), by outpoint from the API. */
  consumes?: { txHash: string; index: number; role: string };
  /** The output that carries the channel to the next step. */
  carries?: { index: number; role: string };
  box: FiberStepBox;
}

const short = (hash: string) => `${hash.slice(0, 10)}…${hash.slice(-6)}`;

function codeHashesOf(tx: TransactionDetail | undefined): string[] {
  if (!tx) return [];
  return Array.from(
    new Set(
      [...(tx.inputs ?? []), ...(tx.outputs ?? [])]
        .flatMap((cell) => [cell.lock?.codeHash, cell.type?.codeHash])
        .filter((hash): hash is string => hash !== undefined)
    )
  );
}

/**
 * One on-chain step of a Fiber channel in the approved txviz structure (FiberChannel board V66):
 * lock names on the tiles, the channel cell named by its role, and the `onward` bridge.
 */
export function FiberChannelStep({
  n,
  title,
  txHash,
  blockNumber,
  timestamp,
  description,
  epoch,
  consumes,
  carries,
  box,
}: Props) {
  const transaction = useQuery({
    queryKey: ['cell-transaction-detail', txHash],
    queryFn: () => api.getTransactionDetail(txHash),
  });
  const codeHashes = codeHashesOf(transaction.data);
  const lookup = useQuery({
    queryKey: ['scriptLookup', codeHashes, txHash],
    queryFn: () => api.lookupScripts(codeHashes, txHash),
    enabled: codeHashes.length > 0,
    staleTime: Infinity,
  });
  const header = (
    <h3 className="flex flex-wrap items-baseline gap-x-3.5 gap-y-1 font-normal">
      <span className="text-text-bright text-[13.5px]">
        {n} · {title}
      </span>
      <span className="text-text-dim text-[12px]">
        <Link href={`/tx/${txHash}`} className="text-[#6aa9c4] hover:underline" title={txHash}>
          {short(txHash)}
        </Link>
        {' · '}
        <Link href={`/blocks/${blockNumber}`} className="hover:text-jade">
          {formatBlockNumber(blockNumber)}
        </Link>
        {` · ${formatUtc8DateTime(timestamp)}`}
        {epoch !== undefined && ` · epoch ${stageEpoch(epoch)}`}
      </span>
    </h3>
  );
  let body: ReactNode;
  if (transaction.isPending || (codeHashes.length > 0 && lookup.isPending)) {
    body = (
      <p className="text-text-dim text-[12px]">Loading the {title.toLowerCase()} transaction…</p>
    );
  } else if (transaction.error || lookup.error) {
    body = (
      <p role="alert" className="text-rouge break-all text-[12px]">
        {title} transaction unavailable: {String(transaction.error ?? lookup.error)}
      </p>
    );
  } else {
    const tx = transaction.data!;
    if (tx.status !== 'committed' || !tx.inputs || !tx.outputs)
      throw new Error(`Fiber ${title} transaction ${txHash} lacks committed structure`);
    const tiles = cellTiles(tx, lookup.data);
    const spentIndex = consumes
      ? tx.inputs.findIndex(
          (input) =>
            input.previousOutput?.txHash === consumes.txHash &&
            input.previousOutput.index === consumes.index
        )
      : -1;
    if (consumes && spentIndex < 0) {
      body = (
        <p role="alert" className="text-rouge break-all text-[12px]">
          {title} transaction {txHash} does not spend the {consumes.role} cell {consumes.txHash}:
          {consumes.index}
        </p>
      );
    } else if (carries && carries.index >= tiles.outputs.length) {
      body = (
        <p role="alert" className="text-rouge break-all text-[12px]">
          {title} transaction {txHash} has no output {carries.index} to carry the channel
        </p>
      );
    } else {
      const inputs = tiles.inputs.map((tile, index) =>
        index === spentIndex ? { ...tile, content: consumes!.role } : tile
      );
      const outputs = tiles.outputs.map((tile, index) =>
        carries && index === carries.index ? { ...tile, content: carries.role } : tile
      );
      const continuity: TxContinuity[] =
        consumes && carries
          ? [
              {
                from: `in:${spentIndex}`,
                to: `out:${carries.index}`,
                label: 'the channel continues',
                kind: 'file',
              },
            ]
          : [];
      body = (
        <div className="grid min-w-0 items-start gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(200px,300px)]">
          <TxStructure
            txHash={txHash}
            inputs={inputs}
            outputs={outputs}
            fee={tx.fee}
            poolCapacity={tx.inputsCapacity ?? undefined}
            continuity={continuity}
            meOut={carries ? `out:${carries.index}` : undefined}
            onward="the channel, onward"
            hideGroups
          />
          <div className="flex flex-col gap-1 rounded-md border border-dashed border-[#4b5266] px-3 py-2.5 text-[12px]">
            <span className="text-text-bright">{box.head}</span>
            {box.lines.map((line, index) => (
              <span key={index} className="text-text-dim">
                {line}
              </span>
            ))}
          </div>
        </div>
      );
    }
  }
  return (
    <div className="flex min-w-0 flex-col gap-2.5">
      {header}
      {description && <p className="text-text text-[12.5px]">{description}</p>}
      {body}
    </div>
  );
}
