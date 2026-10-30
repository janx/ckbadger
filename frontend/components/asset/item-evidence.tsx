'use client';
import { useState } from 'react';
import type { ItemProtocol, ItemFeedContext, Transaction } from '@/lib/api';
import { ProtocolEvidence } from '@/components/asset/protocol-evidence';
import { AppLink } from '@/components/ui/app-link';
import { Hash } from '@/components/ui/hash';
import { ExactCkb } from '@/components/activity-event-row';
import { getScriptDetailHref } from '@/lib/detail-routes';
import { formatBlockNumber } from '@/lib/format-block';
import { itemFeedHeadline, type ItemFeedSubject } from '@/lib/feed-headlines';
import { FeedHeadlineText } from '@/components/asset/feed-headline-text';
import { cn } from '@/lib/utils';
export interface ItemEvidenceProps {
  protocol: ItemProtocol;
}
export function ItemEvidence({ protocol }: ItemEvidenceProps) {
  const [selected, setSelected] = useState<string | null>(null);
  const version =
    selected === null
      ? protocol.versions.at(-1)
      : protocol.versions.find((v) => `${v.cell.txHash}:${v.cell.outputIndex}` === selected);
  if (!version) throw new Error(`${protocol.itemId}: missing selected protocol version`);
  const { cell } = version;
  return (
    <section aria-label="Raw item evidence" className="min-w-0 space-y-5">
      <label className="flex flex-wrap items-center gap-3 text-[11px]">
        Cell version{' '}
        <select
          className="bg-base-bg border-base-border max-w-full border p-2"
          value={`${cell.txHash}:${cell.outputIndex}`}
          onChange={(e) => setSelected(e.target.value)}
        >
          {protocol.versions.map((v, i) => (
            <option
              key={`${v.cell.txHash}:${v.cell.outputIndex}`}
              value={`${v.cell.txHash}:${v.cell.outputIndex}`}
            >
              Version {i + 1} · block {formatBlockNumber(v.cell.blockNumber)}
              {v.cell.isLive ? ' · current' : ''}
            </option>
          ))}
        </select>
      </label>
      <div className="break-all text-[11px]">
        Outpoint{' '}
        <AppLink href={`/cell/${cell.txHash}-${cell.outputIndex}`}>
          <Hash hash={cell.txHash} truncate={false} copyable={false} />:{cell.outputIndex}
        </AppLink>
      </div>
      {[
        { kind: 'lock' as const, script: cell.lock, name: cell.lockName },
        { kind: 'type' as const, script: cell.type, name: cell.typeName },
      ].map(
        ({ kind, script, name }) =>
          script && (
            <details key={kind} className="min-w-0 text-[11px]">
              <summary className="cursor-pointer">
                {kind} script · {name ?? 'Unregistered'}
              </summary>
              <dl className="mt-3 space-y-2">
                <div>
                  <dt>Code hash · {script.hashType}</dt>
                  <dd className="break-all">
                    <AppLink
                      href={getScriptDetailHref({
                        codeHash: script.codeHash,
                        hashType: script.hashType,
                        scriptKind: kind,
                      })}
                    >
                      <Hash hash={script.codeHash} truncate={false} copyable={false} />
                    </AppLink>
                  </dd>
                </div>
                <div>
                  <dt>Args</dt>
                  <dd className="break-all">{script.args}</dd>
                </div>
              </dl>
            </details>
          )
      )}
      <ProtocolEvidence
        title={`${protocol.kind} cell data · version ${protocol.versions.findIndex((v) => v.cell.txHash === cell.txHash && v.cell.outputIndex === cell.outputIndex) + 1}`}
        dataHex={version.dataHex}
        fields={version.fields}
        decoded={version.decoded}
      />
    </section>
  );
}
export function ItemTransactionContext({
  transaction,
  item,
}: {
  transaction: Transaction & { itemContext?: ItemFeedContext };
  /** Names the row's protocol actions as its Activities row does (ID-C4). */
  item: ItemFeedSubject;
}) {
  const context = (transaction as Transaction & { itemContext?: ItemFeedContext }).itemContext;
  if (!context)
    throw new Error(`${transaction.hash}: item transaction is missing its item context`);
  const headline = itemFeedHeadline(
    context,
    transaction.activity?.protocolActions ?? null,
    transaction.hash,
    item
  );
  return (
    <div
      data-acted-on={context.actedOn}
      className={cn(
        'space-y-1 whitespace-normal text-[11px]',
        headline.neighbour && 'opacity-[0.45]'
      )}
    >
      <span className="text-text-bright">
        <FeedHeadlineText headline={headline} />
      </span>
      {context.createdCell ? (
        <>
          <AppLink
            className="block"
            href={`/cell/${context.createdCell.txHash}-${context.createdCell.outputIndex}`}
          >
            Output {context.createdCell.outputIndex} · cell after
          </AppLink>
          <ExactCkb
            shannons={BigInt(context.createdCell.capacity)}
            signed={false}
            tone="plain"
            wrap
          />
        </>
      ) : (
        <span className="text-text-dim block">No resulting item cell</span>
      )}
    </div>
  );
}
