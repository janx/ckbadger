'use client';

import type { ReactNode } from 'react';
import Link from '@/components/ui/link';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { Hash } from '@/components/ui/hash';
import { ClassMark } from '@/components/ui/class-mark';
import { AlignedBar } from '@/components/ui/aligned-bar';
import {
  DotJoined,
  ExactCkb,
  activityClassMark,
  activityHeadline,
  protocolActionNoteParts,
  formatScriptRef,
  ActivityReadError,
} from '@/components/activity-event-row';
import { resourceScale } from '@/components/blocks/block-visual';
import { parseShannons } from '@/lib/ckb-decimal';
import { formatTimeAgo } from '@/lib/utils';
import { formatBlockNumber } from '@/lib/format-block';
import { formatCount } from '@/lib/format-count';
import type { GlobalActivity, Transaction } from '@/lib/api';

const TRACK = 80;

/** A zero amount drawn quiet (board: `#4b5266`), never a different number. */
function Quiet({ when, children }: { when: boolean; children: ReactNode }) {
  return when ? (
    <span data-zero className="[&_[data-ckb-amount]]:text-text-dim">
      {children}
    </span>
  ) : (
    <>{children}</>
  );
}

/** A cellbase's note (R2b-a 35): the block whose reward it pays, or that it pays none. */
function paysNote(tx: Transaction): ReactNode {
  if (tx.pays === undefined) throw new Error(`Cellbase ${tx.hash} has no "pays" field`);
  if (tx.pays === null) return 'pays no reward yet';
  return (
    <>
      pays{' '}
      <Link href={`/blocks/${tx.pays.number}`} className="hover:text-jade">
        {formatBlockNumber(tx.pays.number)}
      </Link>
    </>
  );
}

/** The quiet note beside a row's kind: its protocol facts, its type scripts, or its parties. */
function whatNote(activity: GlobalActivity): ReactNode {
  if (activity.protocolActions.length > 0) {
    try {
      const parts = activity.protocolActions.flatMap((pa) => protocolActionNoteParts(pa));
      return parts.length ? <DotJoined parts={parts} /> : null;
    } catch (error) {
      return <ActivityReadError txHash={activity.txHash} error={error} />;
    }
  }
  const names = [
    ...new Set(
      activity.typeCalls.map((sc) =>
        formatScriptRef({ codeHash: sc.typeCodeHash, scriptName: sc.scriptName })
      )
    ),
  ];
  if (names.length > 0) return names.join(' · ');
  return formatCount(activity.participants.length, 'participant');
}

/** A bar column's header: its name, then the 0 … scale ends of the shared track. */
function ScaleHeader({ label, scale }: { label: string; scale: number }) {
  return (
    <>
      {label}
      <span
        aria-hidden="true"
        className="mt-0.5 flex justify-between text-[9.5px]"
        style={{ width: TRACK }}
      >
        <span>0</span>
        <span>{scale.toLocaleString('en-US')}</span>
      </span>
    </>
  );
}

/**
 * The committed list of the Transactions board (TX-25 / TX-26): general columns first (tx,
 * block · age, what it did, value, fee), then the quieter cells / size / cycles on one shared
 * scale per column. A cellbase row is set back and shows the reward it pays, grey (D5): Value
 * itself excludes cellbase issuance, so the reward is never added to a total.
 */
export function TransactionListTable({
  transactions,
  label = 'Committed transactions',
}: {
  transactions: readonly Transaction[];
  label?: string;
}) {
  for (const tx of transactions) {
    if (!Number.isSafeInteger(tx.txSize) || tx.txSize! <= 0)
      throw new Error(`Transaction ${tx.hash} is missing its exact serialized size`);
    if (tx.cyclesStatus === 'complete' ? tx.cycles === null : tx.cycles !== null)
      throw new Error(`Transaction ${tx.hash} has inconsistent cycle evidence`);
    if (tx.isCellbase ? tx.outputCapacity === undefined : !tx.activity)
      throw new Error(
        `Transaction ${tx.hash} is missing its ${tx.isCellbase ? 'output capacity' : 'activity'}`
      );
  }
  const sizeScale = resourceScale(transactions.map((tx) => tx.txSize!));
  const cycleScale = resourceScale(
    transactions.flatMap((tx) => (tx.cycles === null ? [] : [tx.cycles]))
  );
  const columns: ListColumn<Transaction>[] = [
    {
      key: 'transaction',
      label: 'Transaction',
      minWidth: 160,
      render: (tx) => (
        <Link href={`/tx/${tx.hash}`} className="hover:underline">
          <Hash hash={tx.hash} startChars={10} endChars={6} copyable={false} className="text-xs" />
        </Link>
      ),
    },
    {
      key: 'block',
      label: 'Block · age',
      minWidth: 175,
      render: (tx) => (
        <span className="whitespace-nowrap text-xs">
          <Link href={`/blocks/${tx.blockNumber}`} className="hover:text-jade">
            {formatBlockNumber(tx.blockNumber)}
          </Link>
          <span className="text-text-dim"> · {formatTimeAgo(tx.timestamp)}</span>
        </span>
      ),
    },
    {
      key: 'what',
      label: 'What it did',
      minWidth: 220,
      render: (tx) => {
        const mark = tx.activity ? activityClassMark(tx.activity) : null;
        return (
          <span className="flex min-w-0 items-center gap-2">
            <span className="flex h-4 w-4 flex-none">{mark && <ClassMark kind={mark} />}</span>
            <span className={tx.isCellbase ? 'text-text flex-none' : 'text-text-bright flex-none'}>
              {tx.isCellbase ? 'Cellbase' : activityHeadline(tx.activity!)}
            </span>
            <span className="text-text-dim min-w-0 truncate text-xs">
              {tx.isCellbase ? paysNote(tx) : whatNote(tx.activity!)}
            </span>
          </span>
        );
      },
    },
    {
      key: 'value',
      label: 'Value, CKB',
      minWidth: 170,
      numeric: true,
      render: (tx) =>
        tx.isCellbase ? (
          <span data-reward className="text-text-dim">
            reward{' '}
            <span className="[&_[data-ckb-amount]]:text-text-dim">
              <ExactCkb
                shannons={parseShannons(tx.outputCapacity!, `cellbase ${tx.hash} reward`)}
                signed={false}
                tone="plain"
                unit={false}
              />
            </span>
          </span>
        ) : (
          <Quiet when={parseShannons(tx.value, `transaction ${tx.hash} value`) === BigInt(0)}>
            <ExactCkb
              shannons={parseShannons(tx.value, `transaction ${tx.hash} value`)}
              signed={false}
              tone="plain"
              unit={false}
            />
          </Quiet>
        ),
    },
    {
      key: 'fee',
      label: 'Fee, CKB',
      minWidth: 105,
      numeric: true,
      render: (tx) => (
        <Quiet when={parseShannons(tx.fee, `transaction ${tx.hash} fee`) === BigInt(0)}>
          <ExactCkb
            shannons={parseShannons(tx.fee, `transaction ${tx.hash} fee`)}
            signed={false}
            tone="plain"
            unit={false}
          />
        </Quiet>
      ),
    },
    {
      key: 'cells',
      label: 'Cells',
      minWidth: 56,
      render: (tx) => (
        <span className="text-text-dim text-xs">
          {tx.isCellbase ? 0 : tx.inputsCount} → {tx.outputsCount}
        </span>
      ),
    },
    {
      key: 'size',
      label: 'Size, bytes',
      minWidth: 145,
      header: <ScaleHeader label="Size, bytes" scale={sizeScale} />,
      render: (tx) => (
        <AlignedBar
          label="Size, bytes"
          scale={sizeScale}
          trackWidth={TRACK}
          segments={[
            {
              key: 'value',
              label: 'Size, bytes',
              value: tx.txSize!,
              tone: tx.isCellbase ? 'overhead' : 'data',
            },
          ]}
        />
      ),
    },
    {
      key: 'cycles',
      label: 'Cycles',
      minWidth: 180,
      header: <ScaleHeader label="Cycles" scale={cycleScale} />,
      render: (tx) =>
        tx.cycles === null ? (
          <span className="text-text-dim text-xs">
            {tx.cyclesStatus === 'failed' ? 'replay failed' : 'not yet run'}
          </span>
        ) : (
          <AlignedBar
            label="Cycles"
            scale={cycleScale}
            trackWidth={TRACK}
            segments={[{ key: 'value', label: 'Cycles', value: tx.cycles, tone: 'data' }]}
          />
        ),
    },
  ];
  return (
    <ListTable
      label={label}
      rows={transactions}
      columns={columns}
      rowKey={(tx) => tx.hash}
      rowClassName={(tx) => (tx.isCellbase ? 'opacity-[.55]' : '')}
      empty="No transactions."
    />
  );
}
