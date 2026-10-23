'use client';

import { chipPaint, tok } from '@/lib/pipeline/encoding';
import { G, tail4, type SceneCard } from '@/lib/pipeline/layout';
import { T10, T10C, T10G } from '@/components/pipeline/svg-text';
import { TxSquare } from '@/components/pipeline/tx-square';

export type DimLevel = 'false' | 'true' | 'soft';

interface BlockCardProps {
  card: SceneCard;
  /** The block number is gold while it lies inside the drawn proposal window. */
  numberInWindow: boolean;
  highlighted: boolean;
  dim: DimLevel;
  /** tx hashes kept bright under hover focus. */
  onIds?: ReadonlySet<string>;
}

/**
 * A mined block as a card shaped like a CKB block: header (number, the chain runs through it),
 * the proposal row, the uncles it carries, and the transaction field starting with the cellbase.
 */
export function BlockCard({ card, numberInWindow, highlighted, dim, onIds }: BlockCardProps) {
  const { block } = card;
  const own = card.chips.filter((k) => k.proposal.uncleIndex === null).length;
  return (
    <g
      data-card={card.id}
      data-number={block.number}
      data-slot={card.slot}
      transform={`translate(${card.x},0)`}
      className="cursor-pointer"
    >
      <g data-dim={dim}>
        <rect
          data-box
          x={0.5}
          y={G.cardTop + 0.5}
          width={Math.max(1, card.w - 1)}
          height={Math.max(1, card.h - 1)}
          rx={6}
          strokeWidth={1}
          vectorEffect="non-scaling-stroke"
          fillOpacity={highlighted ? 0.9 : 0.65}
          strokeOpacity={highlighted ? 1 : 0.5}
          style={{ fill: tok('elevated'), stroke: tok('jade') }}
        />
        <line
          x1={1}
          x2={card.w - 1}
          y1={G.cardTop + G.headH + 0.5}
          y2={G.cardTop + G.headH + 0.5}
          strokeWidth={1}
          vectorEffect="non-scaling-stroke"
          style={{ stroke: tok('border') }}
        />
        <line
          x1={G.padX}
          x2={card.w - G.padX}
          y1={G.sepY + 0.5}
          y2={G.sepY + 0.5}
          strokeWidth={1}
          vectorEffect="non-scaling-stroke"
          style={{ stroke: tok('border') }}
        />
        <text
          data-block-number
          x={G.padX}
          y={G.chainY + 3.5}
          className={numberInWindow ? T10G : T10C}
        >
          {tail4(block.number)}
        </text>
        <g data-uncles>
          {card.uncles.map((u) => (
            <g key={u.ref.hash} data-uncle={u.index}>
              <rect
                x={G.padX}
                y={u.y + 0.5}
                width={card.w - 2 * G.padX}
                height={u.h}
                rx={3}
                fillOpacity={0.6}
                strokeOpacity={0.7}
                strokeWidth={1}
                strokeDasharray="2 2"
                vectorEffect="non-scaling-stroke"
                style={{ fill: tok('surface'), stroke: tok('ink-dim') }}
              />
              <text x={G.padX + 3} y={u.y + 9} className={T10}>
                {u.label}
              </text>
              {u.overflow > 0 && (
                <text
                  x={card.w - G.padX - 3}
                  y={u.y + 9}
                  textAnchor="end"
                  className={T10}
                >{`+${u.overflow}`}</text>
              )}
            </g>
          ))}
        </g>
        <g data-chips>
          {card.chips.map((k) => {
            const paint = chipPaint(k.proposal.state);
            return (
              <rect
                key={k.key}
                data-chip={k.key}
                data-state={k.proposal.state}
                data-uncle-index={k.proposal.uncleIndex ?? undefined}
                x={k.x}
                y={k.y}
                width={G.chip}
                height={G.chip}
                rx={1}
                fillOpacity={paint.fillOpacity}
                strokeWidth={paint.stroke ? 1 : undefined}
                vectorEffect="non-scaling-stroke"
                style={{ fill: paint.fill, stroke: paint.stroke ?? 'none' }}
              />
            );
          })}
          {card.chipOverflow > 0 && (
            <text
              data-chip-overflow
              x={card.w - G.padX}
              y={G.sepY - 1}
              textAnchor="end"
              className={T10}
            >{`+${card.chipOverflow}`}</text>
          )}
          {own === 0 && (
            <text x={G.padX} y={G.propTop + 7} className={T10}>
              none
            </text>
          )}
        </g>
        <g data-field>
          {card.squares.map((sq) => (
            <TxSquare key={sq.id} sq={sq} on={onIds?.has(sq.id)} />
          ))}
        </g>
        {card.hidden > 0 && (
          <text data-hidden x={G.padX} y={G.cardTop + card.h + 11} className={T10C}>
            {`+${card.hidden}`}
          </text>
        )}
        <text
          data-count
          x={card.w - 2}
          y={G.cardTop + card.h + 11}
          textAnchor="end"
          className={T10}
        >{`${block.txCount} tx`}</text>
      </g>
    </g>
  );
}
