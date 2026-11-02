'use client';
import { useState } from 'react';
import type { ItemProtocol, ItemFeedContext, ProtocolVersion, Transaction } from '@/lib/api';
import { ProtocolEvidence } from '@/components/asset/protocol-evidence';
import { withFieldMeanings } from '@/lib/protocol-field-meanings';
import { ScriptByteMap } from '@/components/ui/script-byte-map';
import { AppLink } from '@/components/ui/app-link';
import { Hash } from '@/components/ui/hash';
import { ExactCkb } from '@/components/activity-event-row';
import { getScriptDetailHref } from '@/lib/detail-routes';
import { formatBlockNumber } from '@/lib/format-block';
import { itemFeedHeadline, type ItemFeedSubject } from '@/lib/feed-headlines';
import { FeedHeadlineText } from '@/components/asset/feed-headline-text';
import { cn } from '@/lib/utils';
import { OutpointIndex } from '@/components/ui/outpoint-index';
export interface ItemEvidenceProps {
  protocol: ItemProtocol;
  /** F2 identities (round 2, ID-C9): the data map's title for the selected version. */
  title?: (version: ProtocolVersion<unknown>, index: number) => string;
}
export function ItemEvidence({ protocol, title }: ItemEvidenceProps) {
  const [selected, setSelected] = useState<string | null>(null);
  const version =
    selected === null
      ? protocol.versions.at(-1)
      : protocol.versions.find((v) => `${v.cell.txHash}:${v.cell.outputIndex}` === selected);
  if (!version) throw new Error(`${protocol.itemId}: missing selected protocol version`);
  const { cell } = version;
  return (
    <section aria-label="Raw item evidence" className="min-w-0 space-y-5">
      <label className="text-meta flex flex-wrap items-center gap-3">
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
      <div className="text-meta break-all">
        Outpoint{' '}
        <AppLink href={`/cell/${cell.txHash}-${cell.outputIndex}`}>
          <Hash hash={cell.txHash} truncate={false} copyable={false} />
          <OutpointIndex index={cell.outputIndex} />
        </AppLink>
      </div>
      {[
        { kind: 'lock' as const, script: cell.lock, name: cell.lockName },
        { kind: 'type' as const, script: cell.type, name: cell.typeName },
      ].map(
        ({ kind, script, name }) =>
          script && (
            <details key={kind} className="text-meta min-w-0">
              <summary className="cursor-pointer">
                {kind} script · {name ?? 'Unregistered'}
              </summary>
              {/* Lane E (C6): the script as its occupied bytes, one row per part. */}
              <div className="mt-3">
                <ScriptByteMap
                  kind={kind}
                  script={script}
                  name={name}
                  href={getScriptDetailHref({
                    codeHash: script.codeHash,
                    hashType: script.hashType,
                    scriptKind: kind,
                  })}
                />
              </div>
            </details>
          )
      )}
      <ProtocolEvidence
        title={
          title
            ? title(
                version as ProtocolVersion<unknown>,
                protocol.versions.findIndex(
                  (v) => v.cell.txHash === cell.txHash && v.cell.outputIndex === cell.outputIndex
                )
              )
            : `${protocol.kind} cell data · version ${protocol.versions.findIndex((v) => v.cell.txHash === cell.txHash && v.cell.outputIndex === cell.outputIndex) + 1}`
        }
        dataHex={version.dataHex}
        fields={withFieldMeanings(protocol.kind, version.fields)}
        decoded={version.decoded}
      />
    </section>
  );
}
export function ItemTransactionContext({
  transaction,
  item,
  part = 'all',
}: {
  transaction: Transaction & { itemContext?: ItemFeedContext };
  /** Names the row's protocol actions as its Activities row does (ID-C4). */
  item: ItemFeedSubject;
  /** ID-C8: the identity boards give the action and the cell after it a column each. */
  part?: 'all' | 'action' | 'cell';
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
        'text-meta space-y-1 whitespace-normal',
        headline.neighbour && 'opacity-[0.45]'
      )}
    >
      {part !== 'cell' && (
        <span className="text-text-bright">
          <FeedHeadlineText headline={headline} />
        </span>
      )}
      {part === 'action' ? null : context.createdCell ? (
        <>
          {/* F2 objects (OB-13): where the object went, as the ObjectTxs board names it. */}
          {item.kind === 'object' && (
            <span className="text-text-dim block">
              to{' '}
              <AppLink href={`/address/${context.createdCell.ownerAddress}`}>
                {`${context.createdCell.ownerAddress.slice(0, 10)}…${context.createdCell.ownerAddress.slice(-6)}`}
              </AppLink>{' '}
              (output {context.createdCell.outputIndex})
            </span>
          )}
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
