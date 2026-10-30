'use client';

import { useEffect, useRef } from 'react';
import Link from '@/components/ui/link';
import { BlockContents } from '@/components/blocks/block-visual';
import type { Block, BlockProposal, Transaction } from '@/lib/api';
import { getUncleDetailHref } from '@/lib/detail-routes';

interface BlockPositionProps {
  block: Pick<
    Block,
    | 'number'
    | 'hash'
    | 'parentHash'
    | 'timestamp'
    | 'parentTimestamp'
    | 'confirmations'
    | 'transactionSizes'
    | 'proposalsCount'
    | 'unclesCount'
    | 'uncles'
  >;
  transactions?: readonly Pick<Transaction, 'hash' | 'isCellbase' | 'proposalOrigin'>[];
  proposals?: readonly BlockProposal[];
  page: number;
  hasMore: boolean;
  transactionsError?: string;
  proposalsError?: string;
}

/** Canonical proposal endpoints and uncle references from the existing read responses. */
export function BlockPosition({
  block,
  transactions,
  proposals,
  page,
  hasMore,
  transactionsError,
  proposalsError,
}: BlockPositionProps) {
  const viewport = useRef<HTMLDivElement>(null);
  const arcs = new Map<
    string,
    { from: number; to: number; hashes: Set<string>; uncles: Set<string> }
  >();
  const addArc = (from: number, to: number, hash: string, uncle?: string) => {
    if (!Number.isSafeInteger(from) || !Number.isSafeInteger(to) || from < 0 || to <= from)
      throw new Error(
        `Block ${block.number}: invalid proposal endpoints ${from} → ${to} for ${hash}`
      );
    const key = `${from}:${to}`;
    let arc = arcs.get(key);
    if (!arc) {
      arc = { from, to, hashes: new Set(), uncles: new Set() };
      arcs.set(key, arc);
    }
    arc.hashes.add(hash);
    if (uncle) arc.uncles.add(uncle);
  };
  for (const tx of transactions ?? []) {
    if (tx.isCellbase || block.number === 0) continue;
    if (!tx.proposalOrigin)
      throw new Error(`Block ${block.number}: missing proposal origin for ${tx.hash}`);
    const origin = tx.proposalOrigin;
    if (origin.distance !== block.number - origin.blockNumber)
      throw new Error(`Block ${block.number}: proposal distance disagrees for ${tx.hash}`);
    addArc(origin.blockNumber, block.number, tx.hash, origin.uncle?.hash);
  }
  for (const proposal of proposals ?? []) {
    if (proposal.committedTxHash === null && proposal.committedBlockNumber === null) continue;
    if (proposal.committedTxHash === null || proposal.committedBlockNumber === null)
      throw new Error(
        `Block ${block.number}: incomplete commitment for proposal ${proposal.proposalId}`
      );
    addArc(
      block.number,
      proposal.committedBlockNumber,
      proposal.committedTxHash,
      proposal.uncle?.hash
    );
  }
  const links = [...arcs.values()].sort((a, b) => a.from - b.from || a.to - b.to);
  const heights = new Set([block.number]);
  if (block.number > 0) heights.add(block.number - 1);
  if (block.confirmations > 1) heights.add(block.number + 1);
  for (const arc of links) {
    heights.add(arc.from);
    heights.add(arc.to);
  }
  const ordered = [...heights].sort((a, b) => a - b);
  const tileWidth = 148;
  const step = 192;
  const width = Math.max(620, ordered.length * step - 24);
  const tileTop = 54 + links.length * 26;
  const tileBottom = tileTop + 88;
  const height = tileBottom + 36 + block.uncles.length * 94;
  const x = (number: number) => 12 + ordered.indexOf(number) * step;
  const currentX = x(block.number);
  useEffect(() => {
    if (viewport.current)
      viewport.current.scrollLeft = currentX + tileWidth / 2 - viewport.current.clientWidth / 2;
  }, [currentX, block.number]);
  return (
    <div className="min-w-0 space-y-3">
      <p className="text-text-dim text-xs">
        Gold links show proposals that became committed transactions. Dashed links show uncle
        inclusion.
        {(page > 1 || hasMore) && ` Incoming relationships cover transaction page ${page} only.`}
      </p>
      {transactionsError ? (
        <p role="alert" className="text-text-dim text-xs">
          Proposal origins unavailable: {transactionsError}
        </p>
      ) : transactions === undefined ? (
        <p className="text-text-dim text-xs">Reading transaction proposal origins…</p>
      ) : null}
      {proposalsError && (
        <p role="alert" className="text-text-dim text-xs">
          Outgoing commitments unavailable: {proposalsError}
        </p>
      )}
      <div aria-label="Chain position relationships" className="min-w-0 space-y-4 md:hidden">
        <div className="space-y-2 rounded-md bg-[#123029] p-3 text-xs">
          <Link href={`/blocks/${block.hash}`} className="text-text-bright block">
            Current block #{block.number.toLocaleString('en-US')}
          </Link>
          <BlockContents
            transactionSizes={block.transactionSizes}
            proposalsCount={block.proposalsCount}
            unclesCount={block.unclesCount}
          />
        </div>
        <nav aria-label="Chain neighbors" className="flex flex-wrap gap-x-5 gap-y-2 text-xs">
          {block.number > 0 && (
            <Link href={`/blocks/${block.parentHash}`}>
              ← #{(block.number - 1).toLocaleString('en-US')}
            </Link>
          )}
          {block.confirmations > 1 && (
            <Link href={`/blocks/${block.number + 1}`}>
              #{(block.number + 1).toLocaleString('en-US')} →
            </Link>
          )}
        </nav>
        {links.map((arc) => (
          <div
            key={`${arc.from}:${arc.to}`}
            className="border-base-border-subtle space-y-2 border-t pt-3 text-xs"
          >
            <p className="text-gold">
              {arc.hashes.size.toLocaleString('en-US')} proposed → committed · +{arc.to - arc.from}{' '}
              blocks
            </p>
            <p className="flex flex-wrap items-center gap-2">
              <Link href={`/blocks/${arc.from}`}>#{arc.from.toLocaleString('en-US')}</Link>
              <span aria-hidden="true" className="text-text-dim">
                →
              </span>
              <Link href={`/blocks/${arc.to}`}>#{arc.to.toLocaleString('en-US')}</Link>
            </p>
            {arc.uncles.size > 0 && (
              <p className="text-text-dim">
                Includes proposals carried by {arc.uncles.size} uncle(s).
              </p>
            )}
          </div>
        ))}
        {block.uncles.map((uncle, index) => (
          <div
            key={uncle.hash}
            className="border-base-border-subtle min-w-0 space-y-2 border-t border-dashed pt-3 text-xs"
          >
            <p className="text-text-dim">Uncle parent → included uncle</p>
            <Link href={`/blocks/${uncle.parentHash}`} className="block break-all">
              Parent {uncle.parentHash}
            </Link>
            <Link href={getUncleDetailHref(block.number, index)} className="block break-all">
              Uncle #{uncle.number.toLocaleString('en-US')} · {uncle.hash}
            </Link>
            <p className="text-text-dim">
              Included by{' '}
              <Link href={`/blocks/${block.hash}`}>#{block.number.toLocaleString('en-US')}</Link>
            </p>
          </div>
        ))}
      </div>
      <div
        ref={viewport}
        className="hidden max-w-full overflow-x-auto md:block"
        tabIndex={0}
        role="region"
        aria-label="Chain position diagram"
      >
        <svg
          viewBox={`0 0 ${width} ${height}`}
          style={{ minWidth: width, width: '100%', maxWidth: width }}
          role="img"
          aria-label={`Position of block #${block.number.toLocaleString('en-US')}: ${links.length} proposal relationships and ${block.uncles.length} included uncles`}
        >
          {ordered.slice(1).map((number, index) => (
            <g key={number}>
              <path
                d={`M${x(ordered[index]) + tileWidth} ${tileTop + 44}H${x(number)}`}
                stroke="#2c3348"
                fill="none"
              />
              {number - ordered[index] > 1 && (
                <text
                  x={x(number) - 22}
                  y={tileTop + 38}
                  textAnchor="middle"
                  fill="#7c849c"
                  fontSize="11"
                >
                  …
                </text>
              )}
            </g>
          ))}
          {links.map((arc, index) => {
            const from = x(arc.from) + tileWidth / 2;
            const to = x(arc.to) + tileWidth / 2;
            const top = 22 + index * 26;
            return (
              <g
                key={`${arc.from}:${arc.to}`}
                data-proposal-from={arc.from}
                data-proposal-to={arc.to}
              >
                <title>
                  {arc.hashes.size} transactions proposed in #{arc.from} and committed in #{arc.to}
                  {arc.uncles.size
                    ? `; includes proposals carried by ${arc.uncles.size} uncle(s)`
                    : ''}
                </title>
                <path
                  d={`M${from} ${tileTop}C${from} ${top} ${to} ${top} ${to} ${tileTop}`}
                  fill="none"
                  stroke="#f2c55c"
                  strokeWidth="1.5"
                  opacity="0.75"
                />
                <text
                  x={(from + to) / 2}
                  y={top - 4}
                  textAnchor="middle"
                  fill="#a0a8be"
                  fontSize="11"
                >
                  {arc.hashes.size} proposed → committed +{arc.to - arc.from}
                </text>
              </g>
            );
          })}
          {ordered.map((number) => {
            const current = number === block.number;
            const parent = number === block.number - 1;
            const timestamp = current ? block.timestamp : parent ? block.parentTimestamp : null;
            return (
              <g key={number} transform={`translate(${x(number)}, ${tileTop})`}>
                <Link
                  href={`/blocks/${current ? block.hash : parent ? block.parentHash : number}`}
                  aria-label={`Block #${number.toLocaleString('en-US')}`}
                >
                  <rect
                    width={tileWidth}
                    height="88"
                    rx="6"
                    fill={current ? '#123029' : '#131826'}
                  />
                  <text x="10" y="20" fill={current ? '#dee2ec' : '#a0a8be'} fontSize="12">
                    #{number.toLocaleString('en-US')}
                  </text>
                  {timestamp && (
                    <text x="10" y="39" fill="#7c849c" fontSize="11">
                      {new Date(timestamp).toISOString().slice(11, 19)} UTC
                    </text>
                  )}
                </Link>
                {current && (
                  <foreignObject x="10" y="48" width="130" height="40">
                    <BlockContents
                      transactionSizes={block.transactionSizes}
                      proposalsCount={block.proposalsCount}
                      unclesCount={block.unclesCount}
                    />
                  </foreignObject>
                )}
              </g>
            );
          })}
          {block.uncles.map((uncle, index) => {
            const top = tileBottom + 36 + index * 94;
            const parentX = 12;
            const uncleX = 230;
            return (
              <g key={uncle.hash} data-uncle-hash={uncle.hash}>
                <path d={`M${parentX + 178} ${top + 28}H${uncleX}`} stroke="#4b5266" fill="none" />
                <path
                  d={`M${uncleX + 178} ${top + 28}C${uncleX + 216} ${top + 28} ${currentX + 74} ${tileBottom + 12} ${currentX + 74} ${tileBottom}`}
                  fill="none"
                  stroke="#7c849c"
                  strokeDasharray="3 3"
                />
                <Link
                  href={`/blocks/${uncle.parentHash}`}
                  aria-label={`Parent of uncle #${uncle.number.toLocaleString('en-US')}`}
                >
                  <rect x={parentX} y={top} width="178" height="58" rx="6" fill="#10131c" />
                  <text x={parentX + 10} y={top + 21} fill="#7c849c" fontSize="11">
                    Parent #{(uncle.number - 1).toLocaleString('en-US')}
                  </text>
                  <text x={parentX + 10} y={top + 41} fill="#a0a8be" fontSize="11">
                    {uncle.parentHash.slice(0, 12)}…{uncle.parentHash.slice(-6)}
                  </text>
                </Link>
                <Link
                  href={getUncleDetailHref(block.number, index)}
                  aria-label={`Included uncle #${uncle.number.toLocaleString('en-US')}`}
                >
                  <rect
                    x={uncleX}
                    y={top}
                    width="178"
                    height="58"
                    rx="6"
                    fill="none"
                    stroke="#4b5266"
                    strokeDasharray="3 3"
                  />
                  <text x={uncleX + 10} y={top + 21} fill="#a0a8be" fontSize="11">
                    Uncle #{uncle.number.toLocaleString('en-US')}
                  </text>
                  <text x={uncleX + 10} y={top + 41} fill="#7c849c" fontSize="11">
                    {uncle.hash.slice(0, 12)}…{uncle.hash.slice(-6)}
                  </text>
                </Link>
              </g>
            );
          })}
        </svg>
      </div>
    </div>
  );
}
