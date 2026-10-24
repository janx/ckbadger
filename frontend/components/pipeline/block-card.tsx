'use client';

import { TILE_FILL, ageOpacity, chipPaint, tok } from '@/lib/pipeline/encoding';
import { G, tail4, type SceneCard } from '@/lib/pipeline/layout';
import { T10, T11C, T11G } from '@/components/pipeline/svg-text';
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
  /** Blocks behind the tip (0 = the tip): older ones step back, so the eye starts at the newest. */
  age: number;
}

/**
 * A mined block as a solid tile, no outline at rest: its number (the chain runs through it), the
 * proposals (dots: short ids, not txs), the uncles it carries, and the transaction field (squares)
 * starting with the cellbase. The age fade is CSS opacity on an inner group, because the motion
 * layer owns the outer group's `opacity` attribute.
 */
export function BlockCard({ card, numberInWindow, highlighted, dim, onIds, age }: BlockCardProps) {
  const { block } = card;
  return (
    <g
      data-card={card.id}
      data-number={block.number}
      data-slot={card.slot}
      transform={`translate(${card.x},0)`}
      className="cursor-pointer"
    >
      <g data-age={age} style={{ opacity: ageOpacity(age) }}>
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
            fillOpacity={1}
            strokeOpacity={highlighted ? 0.7 : 0}
            style={{ fill: TILE_FILL, stroke: tok('jade') }}
          />
          <text
            data-block-number
            x={G.padX}
            y={G.chainY + 3.5}
            className={numberInWindow ? T11G : T11C}
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
                  style={{ fill: tok('surface') }}
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
                <circle
                  key={k.key}
                  data-chip={k.key}
                  data-state={k.proposal.state}
                  data-uncle-index={k.proposal.uncleIndex ?? undefined}
                  cx={k.x + G.chip / 2}
                  cy={k.y + G.chip / 2}
                  r={paint.stroke ? G.chip / 2 - 0.5 : G.chip / 2}
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
          </g>
          <g data-field>
            {card.squares.map((sq) => (
              <TxSquare key={sq.id} sq={sq} on={onIds?.has(sq.id)} />
            ))}
          </g>
          {card.hidden > 0 && (
            <text
              data-count
              x={card.w - G.padX}
              y={G.chainY + 3.5}
              textAnchor="end"
              className={T10}
            >{`+${card.hidden}`}</text>
          )}
        </g>
      </g>
    </g>
  );
}
