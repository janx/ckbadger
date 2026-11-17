'use client';

import { PLATE_FILL, ageOpacity, chipPaint, tok } from '@/lib/pipeline/encoding';
import { G, tail4, type SceneCard } from '@/lib/pipeline/layout';
import { T10, T10B, T11C, T11G } from '@/components/pipeline/svg-text';
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
 * starting with the cellbase. Older blocks step back by the age fade, CSS opacity on the group of
 * the card's shapes (the motion layer owns the outer group's `opacity` attribute); the card's
 * words (its number, "uncle", the "+N" counts) sit in their own group at full strength in their
 * own inks, because a word is never set back by opacity (R4). The words let the pointer through,
 * so a hover reads the tile, the uncle box or the dot beneath them, as before.
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
      <g data-dim={dim}>
        <g data-age={age} style={{ opacity: ageOpacity(age) }}>
          <rect
            data-box
            x={0.5}
            y={G.cardTop + 0.5}
            width={Math.max(1, card.w - 1)}
            height={Math.max(1, card.h - 1)}
            strokeWidth={1}
            vectorEffect="non-scaling-stroke"
            fillOpacity={1}
            strokeOpacity={highlighted ? 0.7 : 0}
            style={{ fill: PLATE_FILL, stroke: tok('jade') }}
          />
          <g data-uncles>
            {card.uncles.map((u) => (
              <g key={u.ref.hash} data-uncle={u.index}>
                <rect
                  x={G.padX}
                  y={u.y + 0.5}
                  width={card.w - 2 * G.padX}
                  height={u.h}
                  style={{ fill: tok('surface') }}
                />
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
          </g>
          <g data-field>
            {card.squares.map((sq) => (
              <TxSquare key={sq.id} sq={sq} on={onIds?.has(sq.id)} />
            ))}
          </g>
        </g>
        <g data-words pointerEvents="none">
          <text
            data-block-number
            x={G.padX}
            y={G.headY + 3.5}
            className={numberInWindow ? T11G : T11C}
          >
            {tail4(block.number)}
          </text>
          {card.uncles.map((u) => (
            <g key={u.ref.hash} data-uncle-label={u.index}>
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
          {card.chipOverflow > 0 && (
            <g data-chips-label>
              <text
                data-chip-overflow
                x={card.w - G.padX}
                y={G.sepY - 1}
                textAnchor="end"
                className={T10B}
              >{`+${card.chipOverflow}`}</text>
            </g>
          )}
          {card.hidden > 0 && (
            <text
              data-count
              x={card.w - G.padX}
              y={G.headY + 3.5}
              textAnchor="end"
              className={T10B}
            >{`+${card.hidden}`}</text>
          )}
        </g>
      </g>
    </g>
  );
}
