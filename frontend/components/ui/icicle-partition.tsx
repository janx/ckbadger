import type { ReactNode } from 'react';
import { CLASS_KEYS, type AssetClass } from '@/lib/design-tokens';
import { partitionSpans } from '@/lib/partition-geometry';
import { color, inkOnFill, type FillInk, type PaletteKey } from '@/lib/palette';
import { useTheme } from '@/hooks/useTheme';
import { textPx, tightLinePx } from '@/lib/type-metrics';
import { SVG_TEXT, lineAscentPx, wrapWords } from '@/lib/chart-type';

export interface IcicleNode {
  key: string;
  label: string;
  value: bigint;
  tone: AssetClass;
  opacity?: number;
  /** Shown as a dashed box in the next row when this node does not split further. */
  placeholder?: string;
  children?: readonly IcicleNode[];
  // ---- F2 group dao (additive; omitted = the original drawing) ----
  /**
   * The box's fill as a palette key, instead of the tone's class colour (the DAO issuance
   * palette). The words inside a box take the ink its fill calls for (`inkOnFill`), never one
   * picked by hand.
   */
  fill?: PaletteKey;
  /** A quiet third line under the value (or after it, inside the box). */
  note?: string;
  /** Under the box from its left edge (default), under it ending at its right edge, or inside it. */
  align?: 'start' | 'end' | 'inside';
  // ---- hierarchy v3, DAO page (additive; omitted = the labels above) ----
  /**
   * The words inside the box, vertically centred (R12: a graphic names its parts and their shares,
   * never their exact amounts): the name, the share in medium weight and its unit, then a quiet
   * note. Each is written only where it fits the box (R9): all of it, else without the note, else
   * the share alone, else nothing. It replaces the node's other labels.
   */
  boxLabel?: { name: string; share: string; unit?: string; note?: string };
}
export interface IciclePartitionProps {
  root: IcicleNode;
  label: string;
  formatValue: (value: bigint) => ReactNode;
  // ---- F2 group dao (additive) ----
  /** Measured pixel width, so text stays at its set size; default the fixed 900-unit viewBox. */
  width?: number;
  /** Draw only the boxes, rows one box apart; the caller states every word in HTML. */
  hideLabels?: boolean;
  // ---- hierarchy v3, DAO page (additive) ----
  /** Box heights by depth (px) and the gap between rows and between sibling boxes. */
  rows?: { heights: readonly number[]; gap: number };
}

/**
 * A box's paint: its fill (the node's own, else its tone's class colour) as a key and as a colour
 * reference, and its strength.
 */
export function icicleNodePaint(node: IcicleNode): {
  key: PaletteKey;
  color: string;
  opacity: number;
} {
  const key = node.fill ?? CLASS_KEYS[node.tone];
  return { key, color: color(key), opacity: node.opacity ?? 0.9 };
}

/** The class of each ink a word inside a box can take (`FILL_INKS`), as a literal for Tailwind. */
const FILL_INK_CLASS: Record<FillInk, string> = {
  'base-void': 'fill-base-void',
  'text-bright': 'fill-text-bright',
};

const W = 900;
const BOX = 28;
/*
 * The words follow from their roles (readability plan 2026-11-18, lane F3): each is a tight line
 * (lib/chart-type.ts), a line written in a box is centred in it, and the labels under a box are
 * its name (label), its value (value) and its note (note) stacked 3 px under the box: 3 + 15 +
 * 15.5 + 15 and 3 under them, 52 px up to the pixel (was 44, where the three lines met and the
 * last row's note ran past the svg); a row of boxes with labels under them is the box, its labels
 * and 4 px: 84 px as before.
 */
const LABEL_GAP = 3;
const LABEL_Y = LABEL_GAP + lineAscentPx(SVG_TEXT.label);
const VALUE_Y = LABEL_GAP + tightLinePx(SVG_TEXT.label) + lineAscentPx(SVG_TEXT.value);
const NOTE_Y =
  LABEL_GAP +
  tightLinePx(SVG_TEXT.label) +
  tightLinePx(SVG_TEXT.value) +
  lineAscentPx(SVG_TEXT.note);
const LABELS = Math.ceil(
  LABEL_GAP +
    tightLinePx(SVG_TEXT.label) +
    tightLinePx(SVG_TEXT.value) +
    tightLinePx(SVG_TEXT.note) +
    LABEL_GAP
);
const ROW = BOX + LABELS + 4;
/**
 * The lines under the icicle that name a part too narrow for its words (lane P4): 8 px under the
 * last row, each a tight line of the label size (15 px) and 4 px apart, its 9 px swatch centred
 * on the line and its words 6 px after the swatch.
 */
const UNDER_GAP = 8;
const UNDER_LINE = tightLinePx(SVG_TEXT.label);
const UNDER_PITCH = UNDER_LINE + 4;
const UNDER_SWATCH = 9;
const UNDER_WORDS_X = UNDER_SWATCH + 6;
/** A named part's words under the icicle: its name, its share and its unit, and its note. */
const underWords = (label: NonNullable<IcicleNode['boxLabel']>) =>
  `${label.name} ${label.share}${label.unit ? ` ${label.unit}` : ''}${label.note ? ` · ${label.note}` : ''}`;
/** The baseline of a line set at `px` centred in a box `h` tall. */
const centredY = (h: number, px: number) => (h - tightLinePx(px)) / 2 + lineAscentPx(px);

interface Placed {
  node: IcicleNode;
  depth: number;
  x: number;
  width: number;
}

function place(node: IcicleNode, depth: number, x: number, width: number, out: Placed[], gap = 0) {
  out.push({ node, depth, x, width });
  if (!node.children?.length) return;
  // Siblings share the parent's width less the gaps between them, each shifted by its gaps.
  const n = node.children.length;
  const spans = partitionSpans(
    node.children.map((c) => c.value),
    node.value,
    x,
    width - gap * (n - 1),
    node.label
  );
  node.children.forEach((child, i) =>
    place(child, depth + 1, round2(spans[i].x + gap * i), spans[i].width, out, gap)
  );
}

const round2 = (n: number) => Math.round(n * 100) / 100;

/**
 * The words of a `boxLabel` that fit a box `width` by `height` at `px`, padded `pad` on each side:
 * none where the box is lower than their line.
 */
function fittedLabel(
  label: NonNullable<IcicleNode['boxLabel']>,
  width: number,
  height: number,
  px: number,
  pad: number
): { name: boolean; note: boolean } | 'share' | null {
  if (tightLinePx(px) > height) return null;
  const fits = (text: string) => textPx(text.length, px) + 2 * pad <= width;
  const share = `${label.share}${label.unit ? ` ${label.unit}` : ''}`;
  const core = `${label.name} ${share}`;
  if (label.note && fits(`${core} · ${label.note}`)) return { name: true, note: true };
  if (fits(core)) return { name: true, note: false };
  // A share alone keeps its unit (an amount is never a bare number).
  if (fits(share)) return 'share';
  return null;
}

/**
 * An aligned partition (icicle, DAO-7): each row splits the row above exactly, and every part sits
 * under the part it came from. Nodes that stop splitting may leave a dashed note in the next row.
 */
export function IciclePartition({
  root,
  label,
  formatValue,
  width: total = W,
  hideLabels = false,
  rows,
}: IciclePartitionProps) {
  const placed: Placed[] = [];
  place(root, 0, 0, total, placed, rows?.gap ?? 0);
  const depth = Math.max(...placed.map((p) => p.depth));
  if (rows && rows.heights.length !== depth + 1)
    throw new Error(`${label}: ${rows.heights.length} row heights for ${depth + 1} rows`);
  const boxH = (d: number) => (rows ? rows.heights[d] : BOX);
  // Row tops: a row whose words all sit inside its boxes needs no label band below it (F2 dao);
  // otherwise rows stay ROW apart, the original drawing. Set rows sit their gap apart (v3).
  const rowY = [0];
  for (let d = 0; d < depth; d++) {
    const labelsBelow = placed.some((p) => p.depth === d && p.node.align !== 'inside');
    rowY.push(
      rowY[d] +
        (rows ? rows.heights[d] + rows.gap : hideLabels ? BOX + 8 : labelsBelow ? ROW : BOX + 16)
    );
  }
  const lastBelow = placed.some((p) => p.depth === depth && p.node.align !== 'inside');
  // In rows (the DAO page, lane P4: a split bar names every segment in place): a part whose name
  // and share do not fit inside its box writes nothing inside it, and is named on a line of its own
  // under the icicle, at its box: its paint's swatch at the box's left edge and its words after it,
  // or, where they would run past the drawing, ending at the box's right edge (or, narrower still,
  // from the drawing's left edge). One line a part, in the order the parts are placed, so no two
  // meet.
  const fittedOf = (p: Placed) =>
    !hideLabels && p.node.boxLabel
      ? fittedLabel(
          p.node.boxLabel,
          p.width,
          boxH(p.depth),
          boxH(p.depth) >= 40 ? SVG_TEXT.reading : SVG_TEXT.label,
          boxH(p.depth) >= 40 ? 12 : 6
        )
      : null;
  const under = rows
    ? placed.filter((p) => {
        if (hideLabels || !p.node.boxLabel) return false;
        const fitted = fittedOf(p);
        return fitted === null || fitted === 'share';
      })
    : [];
  // Each named part's place and lines: at its box's left edge where its words run on to the
  // drawing's end, else ending at its box's right edge, else from the drawing's left edge,
  // wrapped between words to the drawing's width (a whole wider than a 358 px phone: "secondary
  // issuance since genesis 9,314,713,506 CKB", 50 glyphs, 360 px and the swatch).
  const underTop = rowY[depth] + boxH(depth) + UNDER_GAP;
  let next = underTop;
  const underPlaced = under.map((p) => {
    const words = underWords(p.node.boxLabel!);
    const run = UNDER_WORDS_X + textPx(words.length, SVG_TEXT.label);
    const lines = run <= total ? [words] : wrapWords(words, SVG_TEXT.label, total - UNDER_WORDS_X);
    const x =
      run > total
        ? 0
        : p.x + run <= total
          ? p.x
          : p.x + p.width - run >= 0
            ? p.x + p.width - run
            : 0;
    const top = next;
    next = top + lines.length * UNDER_LINE + (UNDER_PITCH - UNDER_LINE);
    return { p, lines, x, top };
  });
  const height =
    rowY[depth] +
    boxH(depth) +
    (rows || hideLabels || !lastBelow ? 0 : LABELS) +
    (under.length ? next - (UNDER_PITCH - UNDER_LINE) - (rowY[depth] + boxH(depth)) : 0);
  const { resolved: theme } = useTheme();
  return (
    <svg
      role="img"
      aria-label={label}
      viewBox={`0 0 ${total} ${height}`}
      width="100%"
      className="block overflow-visible font-mono"
    >
      {placed.map(({ node, depth: d, x, width }) => {
        const paint = icicleNodePaint(node);
        // Words inside the box: the one ink its fill calls for in this theme, every word of them
        // (a box is a fill, so no word on it is dim or thinned; A5's fill-ink rule, DY-03).
        const boxInk = FILL_INK_CLASS[inkOnFill(theme, paint.key, paint.opacity).ink];
        const h = boxH(d);
        // hierarchy v3: words inside the box at body (a ≥ 40 px row) or meta, where they fit; in
        // rows, a part named under the icicle writes none inside its box (lane P4)
        const px = h >= 40 ? SVG_TEXT.reading : SVG_TEXT.label;
        const pad = h >= 40 ? 12 : 6;
        const named = under.some((p) => p.node === node);
        const fitted = named ? null : fittedOf({ node, depth: d, x, width });
        const end = node.align === 'end';
        const tx = end ? x + width - 10 : x + 2;
        const anchor = end ? 'end' : 'start';
        return (
          <g key={node.key}>
            <rect
              data-node={node.key}
              x={x}
              y={rowY[d]}
              width={width}
              height={h}
              fill={paint.color}
              fillOpacity={paint.opacity}
            />
            {node.boxLabel && fitted !== null && (
              <text
                data-label={node.key}
                x={x + pad}
                y={rowY[d] + centredY(h, px)}
                fontSize={px}
                className={boxInk}
              >
                {fitted === 'share' ? (
                  <>
                    <tspan fontWeight={500}>{node.boxLabel.share}</tspan>
                    {node.boxLabel.unit && ` ${node.boxLabel.unit}`}
                  </>
                ) : (
                  <>
                    {node.boxLabel.name} <tspan fontWeight={500}>{node.boxLabel.share}</tspan>
                    {node.boxLabel.unit && <tspan> {node.boxLabel.unit}</tspan>}
                    {fitted.note && (
                      <tspan data-part="note">
                        {' · '}
                        {node.boxLabel.note}
                      </tspan>
                    )}
                  </>
                )}
              </text>
            )}
            {!hideLabels && !node.boxLabel && node.align === 'inside' && (
              <text
                data-label={node.key}
                x={x + 8}
                y={rowY[d] + centredY(h, SVG_TEXT.head)}
                fontSize={SVG_TEXT.head}
                className={boxInk}
              >
                {node.label} <tspan>{formatValue(node.value)}</tspan>
                {node.note && (
                  <tspan fontSize={SVG_TEXT.label}>
                    {' · '}
                    {node.note}
                  </tspan>
                )}
              </text>
            )}
            {!hideLabels && !node.boxLabel && node.align !== 'inside' && (
              <>
                <text
                  data-label={node.key}
                  x={tx}
                  y={rowY[d] + BOX + LABEL_Y}
                  textAnchor={anchor}
                  fontSize={SVG_TEXT.label}
                  className="fill-text"
                >
                  {node.label}
                </text>
                <text
                  x={tx}
                  y={rowY[d] + BOX + VALUE_Y}
                  textAnchor={anchor}
                  fontSize={SVG_TEXT.value}
                  className="fill-text-bright"
                >
                  {formatValue(node.value)}
                </text>
                {node.note && (
                  <text
                    data-note={node.key}
                    x={tx}
                    y={rowY[d] + BOX + NOTE_Y}
                    textAnchor={anchor}
                    fontSize={SVG_TEXT.note}
                    className="fill-text-dim"
                  >
                    {node.note}
                  </text>
                )}
              </>
            )}
            {node.placeholder && !node.children?.length && d < depth && (
              <g data-placeholder={node.key}>
                <rect
                  x={x + 1}
                  y={rowY[d + 1]}
                  width={Math.max(width - 2, 0)}
                  height={boxH(d + 1)}
                  fill="none"
                  className="stroke-base-border"
                  strokeDasharray="2 3"
                />
                {!hideLabels && (
                  <text
                    x={x + 8}
                    y={rowY[d + 1] + centredY(boxH(d + 1), SVG_TEXT.note)}
                    fontSize={SVG_TEXT.note}
                    className="fill-text-dim"
                  >
                    {node.placeholder}
                  </text>
                )}
              </g>
            )}
          </g>
        );
      })}
      {underPlaced.map(({ p, lines, x, top }) => {
        const paint = icicleNodePaint(p.node);
        return (
          <g key={`under-${p.node.key}`} data-under-label={p.node.key}>
            <rect
              x={x}
              y={top + (UNDER_LINE - UNDER_SWATCH) / 2}
              width={UNDER_SWATCH}
              height={UNDER_SWATCH}
              fill={paint.color}
              fillOpacity={paint.opacity}
            />
            <text
              x={x + UNDER_WORDS_X}
              y={top + lineAscentPx(SVG_TEXT.label)}
              fontSize={SVG_TEXT.label}
              className="fill-text"
            >
              {lines[0]}
              {lines.slice(1).map((line, i) => (
                <tspan key={`${i}:${line}`} x={x + UNDER_WORDS_X} dy={UNDER_LINE}>
                  {line}
                </tspan>
              ))}
            </text>
          </g>
        );
      })}
    </svg>
  );
}
