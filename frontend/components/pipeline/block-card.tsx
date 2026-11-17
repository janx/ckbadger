'use client';

import { useMemo } from 'react';
import { color } from '@/lib/palette';
import { CUT_FILL, PLATE_EDGE, PLATE_FILL, agedChip } from '@/lib/pipeline/encoding';
import { G, cardBox, tail4, type SceneCard } from '@/lib/pipeline/layout';
import { drawSeal } from '@/lib/seal/stones';
import { T10B, T11C, T11G } from '@/components/pipeline/svg-text';
import { TxSquare } from '@/components/pipeline/tx-square';

export type DimLevel = 'false' | 'true' | 'soft';

/**
 * The three inks of a stone pressed into a plate: the cinnabar `mark`, the plate its text is cut
 * in, the dead ink (unused while pressed; `drawSeal` takes all three).
 */
const STONE_PAINTS = {
  ink: color('mark'),
  ground: color('pipeline-plate'),
  dead: color('text-dim'),
} as const;

/**
 * A block's row stone, pressed into the head of its plate at (`x`, `y`), `G.stone` px: the
 * block's own seal (`lib/seal`, its hash and its transaction count), the same one the block's
 * rows carry elsewhere. The SVG is the generator's own markup — numbers, fixed element names, a
 * validated hash, a whole number and three palette references — so, as in `components/ui/seal.tsx`,
 * it is set as inner markup with nothing to sanitise, drawn once per hash and count. It lets the
 * pointer through to the plate.
 */
export function PlateStone({
  hash,
  txs,
  x,
  y,
}: {
  hash: string;
  txs: number;
  x: number;
  y: number;
}) {
  const markup = useMemo(
    () => ({
      __html: drawSeal(
        { kind: 'block', hash, txs },
        { form: 'row', height: G.stone, state: 'pressed', ...STONE_PAINTS }
      ).svg,
    }),
    [hash, txs]
  );
  return (
    <g
      data-stone
      transform={`translate(${x},${y})`}
      pointerEvents="none"
      dangerouslySetInnerHTML={markup}
    />
  );
}

interface BlockCardProps {
  card: SceneCard;
  /** The block number is gold while it lies inside the drawn proposal window. */
  numberInWindow: boolean;
  highlighted: boolean;
  dim: DimLevel;
  /** tx hashes kept bright under hover focus. */
  onIds?: ReadonlySet<string>;
  /** Blocks behind the tip (0 = the tip): an older block's marks step back by their fill. */
  age: number;
}

/**
 * A mined block as a solid plate of its own tone (direction F): no stroke at rest (a 1 px edge in
 * the dim ink while it is highlighted), never faded. In its head the block's stone and its number;
 * under them its proposal dots, the uncles it carries (each its word and its dots: no box, no
 * stone), and its transactions, each square and the cellbase set in its cut — one cut per mark,
 * drawn first, and none where there is no transaction: nothing on a plate stands for what the
 * block does not have.
 *
 * An older block steps back by its marks' fill alone, each mixed toward the ground it stands on
 * (`agedPaint` toward the cut, `agedChip` toward the plate), never by an opacity; the plate, the
 * stone and the words keep full strength, the number in `text-bright`, "uncle" and every "+N" in
 * `text` (the dim ink is 4.1 : 1 on the plate). The words let the pointer through to the plate and
 * the marks beneath them, but an uncle's word, which is what the pointer reads the uncle by. The
 * motion layer owns the outer group's `opacity` (an entrance or an exit) and, while a block lands,
 * its box (NEXT's dashed outline until contact).
 */
export function BlockCard({ card, numberInWindow, highlighted, dim, onIds, age }: BlockCardProps) {
  const { block } = card;
  return (
    <g
      data-card={card.id}
      data-number={block.number}
      data-slot={card.slot}
      data-age={age}
      transform={`translate(${card.x},0)`}
      className="cursor-pointer"
    >
      <g data-dim={dim}>
        <rect
          data-box
          {...cardBox('row', card.w, card.h, highlighted)}
          strokeWidth={1}
          vectorEffect="non-scaling-stroke"
          fillOpacity={1}
          // the whole plate is what a pointer reads the block by, even while it is an outline
          pointerEvents="all"
          style={{ fill: PLATE_FILL, stroke: highlighted ? PLATE_EDGE : 'none' }}
        />
        <PlateStone hash={block.hash} txs={block.txCount} x={G.stoneX} y={G.stoneY} />
        <g data-cuts>
          {card.cuts.map((c) => (
            <rect
              key={c.id}
              data-cut={c.id}
              x={c.x}
              y={c.y}
              width={c.s}
              height={c.s}
              style={{ fill: CUT_FILL }}
            />
          ))}
        </g>
        <g data-field>
          {card.squares.map((sq) => (
            <TxSquare key={sq.id} sq={sq} age={age} on={onIds?.has(sq.id)} />
          ))}
        </g>
        <g data-chips>
          {card.chips.map((k) => {
            const paint = agedChip(k.proposal.state, age);
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
        <g data-words pointerEvents="none">
          <text
            data-block-number
            x={G.numberX}
            y={G.headY + 3.5}
            className={numberInWindow ? T11G : T11C}
          >
            {tail4(block.number)}
          </text>
          {card.uncles.map((u) => (
            <g key={u.ref.hash} data-uncle={u.index} pointerEvents="visiblePainted">
              <text x={u.x} y={u.y + 9} className={T10B}>
                {u.label}
              </text>
              {u.overflow > 0 && (
                <text
                  x={u.x + u.w}
                  y={u.y + 9}
                  textAnchor="end"
                  className={T10B}
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
