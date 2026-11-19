'use client';

import { useEffect, useRef, type ReactNode } from 'react';
import Link from '@/components/ui/link';
import { BLOCK_GLYPH_HEIGHT, BlockContents } from '@/components/blocks/block-visual';
import { SentenceNodes, heightListParts, type SentencePart } from '@/components/ui/block-link';
import type { Block, BlockProposal, Transaction } from '@/lib/api';
import { BLOCK_POSITION_COLORS, CLASS_COLORS, DATA_COLOR } from '@/lib/design-tokens';
import { getUncleDetailHref } from '@/lib/detail-routes';
import { formatBlockNumber } from '@/lib/format-block';
import { formatUtc8Time } from '@/lib/utils';
import { formatCount } from '@/lib/format-count';
import { HexCode } from '@/components/ui/hex-code';
import { Dotted } from '@/components/ui/phone-row';
import { shortenHash } from '@/lib/shorten';
import { unitNode } from '@/components/ui/units';
import { SVG_TEXT, lineAscentPx } from '@/lib/chart-type';
import { descentPx, tightLinePx } from '@/lib/type-metrics';

/*
 * The diagram's words (readability plan 2026-11-18, lane F3): a box holds its lines PAD clear of
 * its top and foot and GAP apart, each the tight line of its role (lib/chart-type.ts), so every
 * box's height and every baseline follow from the roles.
 */
const PAD = 10;
const GAP = 4;
const LABEL_LINE = tightLinePx(SVG_TEXT.label);
const NUMBER_LINE = tightLinePx(SVG_TEXT.head);
/** A block tile: its number (the head), its time (a label), then the contents glyph. */
const NUMBER_Y = PAD + lineAscentPx(SVG_TEXT.head);
const TIME_Y = PAD + NUMBER_LINE + GAP + lineAscentPx(SVG_TEXT.label);
const CONTENTS_Y = PAD + NUMBER_LINE + GAP + LABEL_LINE + GAP;
/** 10 + 16 + 4 + 15 + 4 + 18 + 10 = 77 px. */
export const BLOCK_TILE_HEIGHT = CONTENTS_Y + BLOCK_GLYPH_HEIGHT + PAD;
/** A box of `n` label lines (an uncle, the parent off the strip): 3 lines 73 px, 2 lines 54. */
const boxHeight = (n: number) => PAD + n * LABEL_LINE + (n - 1) * GAP + PAD;
/** The baseline of a box's line `i`. */
const lineY = (i: number) => PAD + i * (LABEL_LINE + GAP) + lineAscentPx(SVG_TEXT.label);
/** A row of uncles: an uncle forked from the tile beside it (three lines) and 22 px for the links. */
const UNCLE_ROW = boxHeight(3) + 22;
/**
 * The proposal arcs, a row each over the tiles: an arc's label is a label line standing on the
 * arc's control line, and a row is that line and the 11 px the arc's crest rises under it; under
 * the rows, the lowest arc's label line and 32 px of its climb to the tiles.
 */
const ARC_PAD = 7;
const ARC_ROW = LABEL_LINE + 11;
const ARC_RISE = LABEL_LINE + 32;

/** A canonical block beside this one, as the blocks list reports it (F2 blocks, BA-15). */
export type BlockNeighbour = Pick<
  Block,
  'number' | 'hash' | 'timestamp' | 'transactionSizes' | 'proposalsCount' | 'unclesCount'
>;

/** "committed what #930 proposed, proposed what #934 committed, and included 1 uncle". */
export function positionParts(
  block: Pick<Block, 'number' | 'proposalsCount' | 'unclesCount'>,
  links: readonly { from: number; to: number }[]
): SentencePart[] {
  const origins = [...new Set(links.filter((l) => l.to === block.number).map((l) => l.from))];
  const targets = [...new Set(links.filter((l) => l.from === block.number).map((l) => l.to))];
  return [
    ...(origins.length
      ? ['committed what ', ...heightListParts(origins.sort((a, b) => a - b)), ' proposed']
      : ['committed no proposed transactions']),
    ', ',
    ...(block.proposalsCount === 0
      ? ['proposed nothing']
      : targets.length
        ? ['proposed what ', ...heightListParts(targets.sort((a, b) => a - b)), ' committed']
        : [`proposed ${formatCount(block.proposalsCount, 'transaction')}, none committed yet`]),
    ', ',
    `and included ${block.unclesCount ? formatCount(block.unclesCount, 'uncle') : 'no uncle'}`,
  ];
}

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
  /** The canonical blocks around this one (two either side), for their times and contents. */
  neighbours?: readonly BlockNeighbour[];
  page: number;
  hasMore: boolean;
  transactionsError?: string;
  proposalsError?: string;
}

/**
 * The diagram's key (lane P1, the plan: "Position in the chain" gets the legend it lacks; review
 * chain-a block-busy): each mark the tiles and the arcs draw, a swatch of the same mark (the
 * contents glyph's diamond, square and dot, the proposal arc's gold, the uncle box's dashed
 * rule) beside its words, labels in the quiet ink. It stands over both forms of the position:
 * the phone's list draws the same glyph and the same gold.
 */
function PositionLegend() {
  const swatch = (mark: ReactNode) => (
    <svg aria-hidden="true" width="18" height="12" viewBox="0 0 18 12" className="shrink-0">
      {mark}
    </svg>
  );
  const items: [string, ReactNode][] = [
    ['the cellbase', <path key="d" d="M9 1.5 13.5 6 9 10.5 4.5 6z" fill={DATA_COLOR} />],
    [
      'a transaction, by its size',
      <rect key="s" x="4" y="1" width="10" height="10" fill={DATA_COLOR} opacity="0.8" />,
    ],
    ['a proposal', <circle key="p" cx="9" cy="6" r="2.4" fill={CLASS_COLORS.dao} />],
    [
      'proposed, then committed',
      <path
        key="a"
        d="M1 11C1 2 17 2 17 11"
        fill="none"
        stroke="var(--c-gold)"
        strokeWidth="1.5"
      />,
    ],
    [
      'an included uncle',
      <rect
        key="u"
        x="1.5"
        y="1.5"
        width="15"
        height="9"
        fill="none"
        stroke="var(--c-ink-quiet)"
        strokeDasharray="3 3"
      />,
    ],
  ];
  return (
    <ul
      aria-label="Chain position legend"
      className="text-text-dim text-meta flex flex-wrap gap-x-5 gap-y-1.5"
    >
      {items.map(([words, mark]) => (
        <li key={words} className="flex items-center gap-1.5">
          {swatch(mark)}
          {words}
        </li>
      ))}
    </ul>
  );
}

/** Canonical proposal endpoints and uncle references from the existing read responses. */
export function BlockPosition({
  block,
  transactions,
  proposals,
  neighbours,
  page,
  hasMore,
  transactionsError,
  proposalsError,
}: BlockPositionProps) {
  const known = new Map((neighbours ?? []).map((n) => [n.number, n]));
  const own = known.get(block.number);
  if (own && own.hash.toLowerCase() !== block.hash.toLowerCase())
    throw new Error(
      `Block ${formatBlockNumber(block.number)}: the blocks list has another block at this height`
    );
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
  for (const n of known.keys()) if (Math.abs(n - block.number) <= 2) heights.add(n);
  // The uncle forks off its parent's tile when that parent is the canonical block beside it.
  const forkFrom = block.uncles.map((uncle) => {
    const parent = known.get(uncle.number - 1);
    return parent && parent.hash.toLowerCase() === uncle.parentHash.toLowerCase()
      ? uncle.number - 1
      : null;
  });
  for (const [index, from] of forkFrom.entries())
    if (from !== null) {
      heights.add(from);
      heights.add(block.uncles[index].number);
    }
  const ordered = [...heights].sort((a, b) => a - b);
  const tileWidth = 148;
  const step = 192;
  const width = Math.max(620, ordered.length * step - 24);
  const tileTop = ARC_PAD + links.length * ARC_ROW + ARC_RISE;
  const tileBottom = tileTop + BLOCK_TILE_HEIGHT;
  const tileMiddle = tileTop + BLOCK_TILE_HEIGHT / 2;
  const height = tileBottom + 36 + block.uncles.length * UNCLE_ROW;
  const x = (number: number) => 12 + ordered.indexOf(number) * step;
  const currentX = x(block.number);
  useEffect(() => {
    if (viewport.current)
      viewport.current.scrollLeft = currentX + tileWidth / 2 - viewport.current.clientWidth / 2;
  }, [currentX, block.number]);
  return (
    <div className="min-w-0 space-y-3">
      {/* statements in the reading ink: the position, and a read that failed; the line being read
          is quiet, a transient line (R3; lane P1 follow-up) */}
      <p data-testid="block-position-sentence" className="text-text text-small max-w-measure">
        <SentenceNodes parts={positionParts(block, links)} />
        {(page > 1 || hasMore) && `. Incoming relationships cover transaction page ${page} only.`}
      </p>
      {transactionsError ? (
        <p role="alert" className="text-text text-small max-w-measure">
          Proposal origins unavailable: {transactionsError}
        </p>
      ) : transactions === undefined ? (
        <p className="text-text-dim text-small">Reading transaction proposal origins…</p>
      ) : null}
      {proposalsError && (
        <p role="alert" className="text-text text-small max-w-measure">
          Outgoing commitments unavailable: {proposalsError}
        </p>
      )}
      <div aria-label="Chain position relationships" className="min-w-0 space-y-4 md:hidden">
        <div
          className="text-small space-y-2 p-3"
          style={{ backgroundColor: BLOCK_POSITION_COLORS.currentTile }}
        >
          <Link look="key" href={`/blocks/${block.hash}`} className="block">
            Current block {formatBlockNumber(block.number)}
          </Link>
          <BlockContents
            transactionSizes={block.transactionSizes}
            proposalsCount={block.proposalsCount}
            unclesCount={block.unclesCount}
          />
        </div>
        <nav aria-label="Chain neighbors" className="text-small flex flex-wrap gap-x-5 gap-y-2">
          {block.number > 0 && (
            <Link look="inline" href={`/blocks/${block.parentHash}`}>
              ← {formatBlockNumber(block.number - 1)}
            </Link>
          )}
          {block.confirmations > 1 && (
            <Link look="inline" href={`/blocks/${block.number + 1}`}>
              {formatBlockNumber(block.number + 1)} →
            </Link>
          )}
        </nav>
        {links.map((arc) => (
          <div
            key={`${arc.from}:${arc.to}`}
            className="border-row-rule text-small space-y-2 border-t pt-3"
          >
            <p className="text-gold max-w-measure">
              {arc.hashes.size.toLocaleString('en-US')} proposed → committed · +
              {unitNode(formatCount(arc.to - arc.from, 'block'))}
            </p>
            <p className="flex flex-wrap items-center gap-2">
              <Link look="inline" href={`/blocks/${arc.from}`}>
                {formatBlockNumber(arc.from)}
              </Link>
              <span aria-hidden="true" className="text-text-dim">
                →
              </span>
              <Link look="inline" href={`/blocks/${arc.to}`}>
                {formatBlockNumber(arc.to)}
              </Link>
            </p>
            {arc.uncles.size > 0 && (
              <p className="text-text max-w-measure">
                Includes proposals carried by {unitNode(formatCount(arc.uncles.size, 'uncle'))}.
              </p>
            )}
          </div>
        ))}
        {block.uncles.map((uncle, index) => (
          <div
            key={uncle.hash}
            className="border-row-rule text-small min-w-0 space-y-2 border-t border-dashed pt-3"
          >
            <p className="text-text-dim">Uncle parent → included uncle</p>
            {/* U-16: a full hash is one token, whole on its line or scrolled in its own box */}
            <Link look="inline" href={`/blocks/${uncle.parentHash}`} className="block">
              Parent <HexCode value={uncle.parentHash} truncate={false} variant="inherit" />
            </Link>
            <Link look="inline" href={getUncleDetailHref(block.number, index)} className="block">
              <Dotted
                parts={[
                  `Uncle ${formatBlockNumber(uncle.number)}`,
                  <HexCode key="hash" value={uncle.hash} truncate={false} variant="inherit" />,
                ]}
              />
            </Link>
            <p className="text-text max-w-measure">
              Included by{' '}
              <Link look="inline" href={`/blocks/${block.hash}`}>
                {formatBlockNumber(block.number)}
              </Link>
            </p>
          </div>
        ))}
      </div>
      <PositionLegend />
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
          aria-label={`Position of block ${formatBlockNumber(block.number)}: ${links.length} proposal relationships and ${block.uncles.length} included uncles`}
        >
          {ordered.slice(1).map((number, index) => (
            <g key={number}>
              <path
                d={`M${x(ordered[index]) + tileWidth} ${tileMiddle}H${x(number)}`}
                stroke="var(--c-slate-other)"
                fill="none"
              />
              {number - ordered[index] > 1 && (
                <text
                  x={x(number) - 22}
                  y={tileMiddle - GAP - descentPx(SVG_TEXT.label)}
                  textAnchor="middle"
                  fill="var(--c-text-dim)"
                  fontSize={SVG_TEXT.label}
                >
                  …
                </text>
              )}
            </g>
          ))}
          {links.map((arc, index) => {
            const from = x(arc.from) + tileWidth / 2;
            const to = x(arc.to) + tileWidth / 2;
            // the arc's control line, the foot of its label's line
            const top = ARC_PAD + LABEL_LINE + index * ARC_ROW;
            return (
              <g
                key={`${arc.from}:${arc.to}`}
                data-proposal-from={arc.from}
                data-proposal-to={arc.to}
              >
                <title>
                  {formatCount(arc.hashes.size, 'transaction')} proposed in{' '}
                  {formatBlockNumber(arc.from)} and committed in {formatBlockNumber(arc.to)}
                  {arc.uncles.size
                    ? `; includes proposals carried by ${arc.uncles.size} uncle(s)`
                    : ''}
                </title>
                <path
                  d={`M${from} ${tileTop}C${from} ${top} ${to} ${top} ${to} ${tileTop}`}
                  fill="none"
                  stroke="var(--c-gold)"
                  strokeWidth="1.5"
                  opacity="0.75"
                />
                <text
                  x={(from + to) / 2}
                  y={top - descentPx(SVG_TEXT.label)}
                  textAnchor="middle"
                  fill="var(--c-text)"
                  fontSize={SVG_TEXT.label}
                >
                  {arc.hashes.size} proposed → committed +{arc.to - arc.from}
                </text>
              </g>
            );
          })}
          {ordered.map((number) => {
            const current = number === block.number;
            const parent = number === block.number - 1;
            const neighbour = known.get(number);
            const timestamp = current
              ? block.timestamp
              : (neighbour?.timestamp ?? (parent ? block.parentTimestamp : null));
            const contents = current ? block : neighbour;
            return (
              <g key={number} transform={`translate(${x(number)}, ${tileTop})`}>
                <Link
                  look="block"
                  href={`/blocks/${current ? block.hash : parent ? block.parentHash : number}`}
                  aria-label={`Block ${formatBlockNumber(number)}`}
                >
                  {/* A tile is the surface under its own 1 px border (DY-08: on the tier-2 ground
                      the tile's fill alone is 1.03:1 by day), drawn on the pixel grid. */}
                  <rect
                    x="0.5"
                    y="0.5"
                    width={tileWidth - 1}
                    height={BLOCK_TILE_HEIGHT - 1}
                    fill={current ? BLOCK_POSITION_COLORS.currentTile : 'var(--c-tile)'}
                    stroke="var(--c-slate-other)"
                  />
                  <text
                    x={PAD}
                    y={NUMBER_Y}
                    fill={current ? 'var(--c-text-bright)' : 'var(--c-text)'}
                    fontSize={SVG_TEXT.head}
                  >
                    {formatBlockNumber(number)}
                  </text>
                  {timestamp && (
                    <text
                      data-tile-time
                      x={PAD}
                      y={TIME_Y}
                      fill="var(--c-text-dim)"
                      fontSize={SVG_TEXT.label}
                    >
                      {formatUtc8Time(timestamp)}
                    </text>
                  )}
                </Link>
                {contents && (
                  <foreignObject
                    x={PAD}
                    y={CONTENTS_Y}
                    width={tileWidth - 2 * PAD}
                    height={BLOCK_GLYPH_HEIGHT}
                  >
                    {/* a flex item, so the glyph is its own height and no line box stands under it */}
                    <div className="flex">
                      <BlockContents
                        transactionSizes={contents.transactionSizes}
                        proposalsCount={contents.proposalsCount}
                        unclesCount={contents.unclesCount}
                        showCount={false}
                      />
                    </div>
                  </foreignObject>
                )}
              </g>
            );
          })}
          {block.uncles.map((uncle, index) => {
            const top = tileBottom + 36 + index * UNCLE_ROW;
            const from = forkFrom[index];
            if (from !== null) {
              const uncleX = x(uncle.number);
              return (
                <g key={uncle.hash} data-uncle-hash={uncle.hash}>
                  <path
                    data-uncle-fork-from={from}
                    d={`M${x(from) + tileWidth} ${tileMiddle}C${x(from) + tileWidth + 25} ${tileMiddle} ${uncleX - 25} ${top + boxHeight(3) / 2} ${uncleX} ${top + boxHeight(3) / 2}`}
                    stroke="var(--c-slate-other)"
                    strokeWidth="1.5"
                    fill="none"
                  />
                  <path
                    d={`M${currentX + tileWidth / 2} ${tileBottom}C${currentX + tileWidth / 2} ${top + boxHeight(3) / 2} ${uncleX + tileWidth + 36} ${top + boxHeight(3) / 2} ${uncleX + tileWidth} ${top + boxHeight(3) / 2}`}
                    fill="none"
                    stroke="var(--c-text-dim)"
                    strokeDasharray="3 3"
                  />
                  <Link
                    look="block"
                    href={getUncleDetailHref(block.number, index)}
                    aria-label={`Included uncle ${formatBlockNumber(uncle.number)}`}
                  >
                    <rect
                      x={uncleX + 0.5}
                      y={top + 0.5}
                      width={tileWidth - 1}
                      height={boxHeight(3) - 1}
                      fill="none"
                      stroke="var(--c-ink-quiet)"
                      strokeDasharray="3 3"
                    />
                    <text
                      x={uncleX + PAD}
                      y={top + lineY(0)}
                      fill="var(--c-text)"
                      fontSize={SVG_TEXT.label}
                    >
                      uncle {formatBlockNumber(uncle.number)}
                    </text>
                    <text
                      x={uncleX + PAD}
                      y={top + lineY(1)}
                      fill="var(--c-text-dim)"
                      fontSize={SVG_TEXT.label}
                    >
                      {shortenHash(uncle.hash, 'tight')}
                    </text>
                    <text
                      data-uncle-time
                      x={uncleX + PAD}
                      y={top + lineY(2)}
                      fill="var(--c-text-dim)"
                      fontSize={SVG_TEXT.label}
                    >
                      {formatUtc8Time(uncle.timestamp)}
                    </text>
                  </Link>
                </g>
              );
            }
            const parentX = 12;
            const uncleX = 230;
            return (
              <g key={uncle.hash} data-uncle-hash={uncle.hash}>
                <path
                  d={`M${parentX + 178} ${top + boxHeight(2) / 2}H${uncleX}`}
                  stroke="var(--c-ink-quiet)"
                  fill="none"
                />
                <path
                  d={`M${uncleX + 178} ${top + boxHeight(2) / 2}C${uncleX + 216} ${top + boxHeight(2) / 2} ${currentX + 74} ${tileBottom + 12} ${currentX + 74} ${tileBottom}`}
                  fill="none"
                  stroke="var(--c-text-dim)"
                  strokeDasharray="3 3"
                />
                <Link
                  look="block"
                  href={`/blocks/${uncle.parentHash}`}
                  aria-label={`Parent of uncle ${formatBlockNumber(uncle.number)}`}
                >
                  <rect
                    x={parentX}
                    y={top}
                    width="178"
                    height={boxHeight(2)}
                    fill="var(--c-base-surface)"
                  />
                  <text
                    x={parentX + PAD}
                    y={top + lineY(0)}
                    fill="var(--c-text-dim)"
                    fontSize={SVG_TEXT.label}
                  >
                    Parent {formatBlockNumber(uncle.number - 1)}
                  </text>
                  <text
                    x={parentX + PAD}
                    y={top + lineY(1)}
                    fill="var(--c-text)"
                    fontSize={SVG_TEXT.label}
                  >
                    {/* SVG labels in 178 px tiles: the tight size (U-09) */}
                    {shortenHash(uncle.parentHash, 'tight')}
                  </text>
                </Link>
                <Link
                  look="block"
                  href={getUncleDetailHref(block.number, index)}
                  aria-label={`Included uncle ${formatBlockNumber(uncle.number)}`}
                >
                  <rect
                    x={uncleX}
                    y={top}
                    width="178"
                    height={boxHeight(2)}
                    fill="none"
                    stroke="var(--c-ink-quiet)"
                    strokeDasharray="3 3"
                  />
                  <text
                    x={uncleX + PAD}
                    y={top + lineY(0)}
                    fill="var(--c-text)"
                    fontSize={SVG_TEXT.label}
                  >
                    Uncle {formatBlockNumber(uncle.number)}
                  </text>
                  <text
                    x={uncleX + PAD}
                    y={top + lineY(1)}
                    fill="var(--c-text-dim)"
                    fontSize={SVG_TEXT.label}
                  >
                    {shortenHash(uncle.hash, 'tight')}
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
