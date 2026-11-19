'use client';
import { withNameBy } from '@/lib/lock-words';
import { HeadId } from '@/components/ui/head-id';
import { useState } from 'react';
import type { ItemProtocol, ItemFeedContext, ProtocolVersion, Transaction } from '@/lib/api';
import { ProtocolEvidence } from '@/components/asset/protocol-evidence';
import { withFieldMeanings } from '@/lib/protocol-field-meanings';
import { ScriptByteMap } from '@/components/ui/script-byte-map';
import { AppLink } from '@/components/ui/app-link';
import { CkbAmount } from '@/components/ui/ckb-amount';
import { getScriptDetailHref } from '@/lib/detail-routes';
import { formatBlockNumber } from '@/lib/format-block';
import { itemFeedHeadline, type ItemFeedSubject } from '@/lib/feed-headlines';
import { FeedHeadlineText } from '@/components/asset/feed-headline-text';
import { cn } from '@/lib/utils';
import { ShortId } from '@/components/ui/short-id';
import { Select } from '@/components/ui/select';
import { cellLockWords } from '@/components/ui/lock-kind';
import { PartReadError, readPart } from '@/components/read-failure';
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
  const lock = readPart(() => {
    const words = cellLockWords(cell);
    return words.form === 'not-indexed' ? null : words.name;
  });
  return (
    <section aria-label="Raw item evidence" className="min-w-0 space-y-5">
      {/* the controls board's select, after its label; the well no wider than the column. A
          native select is as wide as its widest option and clips its value where its well is
          narrower, so an option is the version's number, its block and "current", no word the
          label says: "12 · #21,016,901 · current", 26 glyphs, 202.8 px at 13 px, the pair 304.4 px
          with the label "version" (54.6), the gap (10), the well's border (2) and the select's
          padding (35), inside a 360 px phone's 328 px line up to a five-digit version */}
      <Select
        label="version"
        value={`${cell.txHash}:${cell.outputIndex}`}
        options={protocol.versions.map((v, i) => ({
          value: `${v.cell.txHash}:${v.cell.outputIndex}`,
          label: `${i + 1} · ${formatBlockNumber(v.cell.blockNumber)}${v.cell.isLive ? ' · current' : ''}`,
        }))}
        onChange={setSelected}
        className="max-w-full"
      />
      {/* Lane P2's follow-up (item 8): the out point's hash is set as a head's id (HeadId), one
          line where its line holds it, else two halves, never a box that scrolls. At the line's
          11 px it is 66 × 6.6 = 435.6 px: one line in the tier's column at 1280 (588 px) and 1024
          (460), and at 640 (608); two halves of 34 characters (224.4 px) at 390 (358) and 360
          (328). Its index is said in words before it, the link opening the cell. */}
      <div className="text-meta min-w-0">
        <span className="text-text-dim">Outpoint · output {cell.outputIndex} of</span>
        <AppLink look="inline" href={`/cell/${cell.txHash}-${cell.outputIndex}`}>
          <HeadId value={cell.txHash} kind="hash" />
        </AppLink>
      </div>
      {/* the lock script's name is the one decision's (lane P2's follow-up, item 6): a lock on the
          default lock's code hash by data has no registered name; a refusal fails in place */}
      {lock.error !== undefined && (
        <PartReadError
          what="the lock script"
          where={`cell ${cell.txHash}:${cell.outputIndex}`}
          error={lock.error}
        />
      )}
      {[
        ...(lock.error === undefined
          ? [{ kind: 'lock' as const, script: cell.lock, name: lock.value }]
          : []),
        {
          kind: 'type' as const,
          script: cell.type,
          name: cell.typeName && withNameBy(cell.typeName, cell.typeNameBy),
        },
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
        // a neighbour's row is set back (R4): every word dim, nothing faded
        headline.neighbour && 'quiet'
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
              <AppLink look="inline" href={`/address/${context.createdCell.ownerAddress}`}>
                <ShortId value={context.createdCell.ownerAddress} kind="address" size="row" />
              </AppLink>{' '}
              (output {context.createdCell.outputIndex})
            </span>
          )}
          <AppLink
            look="inline"
            className="block"
            href={`/cell/${context.createdCell.txHash}-${context.createdCell.outputIndex}`}
          >
            Output {context.createdCell.outputIndex} · cell after
          </AppLink>
          <CkbAmount
            form="exact"
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
