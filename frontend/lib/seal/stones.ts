/**
 * The seals by kind: four stones whose outline says what a thing is, whose marks say which one it
 * is and what it holds (rounds 16 and 17 of the rubbing × Song print design, the user's "其他 ok"
 * and "good").
 *
 * - block: a square. Page: the glyph and, in the same column, how many transactions in counting
 *   rods; in the other column two or three written textures, ruled with the brush. Row: the glyph
 *   over a line over the count (to nine; a full block beyond).
 * - transaction: a long rectangle. Rods at the left for the cells it consumed, its round core in
 *   the middle (shaped rim, written border, hair line, glyph), rods at the right for the cells it
 *   created. Row: a slab, rods, the glyph in the rim's cut outline, rods.
 * - cell: round, cut from its transaction's core. Page: that core's rim and border; its index in
 *   rods where the border opens at the foot; a bead on the rim for each decimal digit of its
 *   capacity in whole CKB, clockwise from its owner's small stone (the lock's glyph, at the
 *   owner's hour); the window of its type's class and the glyph of its type's hash. No type: no
 *   window, no glyph, a bare stone. Row: the rim's silhouette, the window, the glyph, the beads
 *   as small cuts, the owner as one upright mark.
 * - script (a lock is an address): tall and rounded. Page: the glyph and one written texture, a
 *   brush rule between. Row: the glyph.
 *
 * The state is the ink: `pressed` in the ink (cinnabar), `ink` in the dead ink (a consumed cell,
 * an uncle), `unpressed` a dashed stone with its field at half strength (not yet committed).
 *
 * Ported from the design prototype's `kinds5.js` (`seal`) expression for expression. A page seal
 * is written in the prototype's markup and is byte-identical to the prototype's but for its label;
 * a row seal draws the same plan through the compact encoding (`encode.ts`, 0.2 px). The fidelity
 * test (`__tests__/lib/seal-fidelity.test.ts`) holds both to the prototype.
 *
 * The markup is the generator's own: fixed element names, numbers, the three paints (validated by
 * `assertPaint`), the tilt (a finite number) and, in the label, validated hashes, whole numbers
 * and a class name. Nothing else enters it, so it can be set as inner HTML.
 */

import { exactCkbText, SHANNONS_PER_CKB } from '@/lib/ckb-decimal';
import type { AssetClass } from '@/lib/design-tokens';
import { BitReader, SEAL_HASH, valueOf } from '@/lib/seal/bits';
import { f1, f2, orderStrokes } from '@/lib/seal/brush';
import {
  coreOf,
  cutGaps,
  hourAngle,
  ownerOf,
  RIM_MIN,
  rimPoints,
  rimR,
  ringStrokes,
  TAU,
  type Gap,
  type RimName,
} from '@/lib/seal/core';
import { compactEncoding, outlinesOf, PROTOTYPE, type Encoding, type Hand } from '@/lib/seal/draw';
import { CUT, glyphOf, type ChosenGlyph } from '@/lib/seal/glyphs';
import { assertPaint } from '@/lib/seal/paint';
import { decimalDigits, ROD, rodCount, rodDigit, type Box } from '@/lib/seal/rods';
import {
  TEXTURE,
  TEXTURES,
  type BrushSpec,
  type FigureStroke,
  type TextureName,
} from '@/lib/seal/textures';
import { isAssetClass, windowOf } from '@/lib/seal/window';

/* ---------- the ids ---------- */

/** The class of a cell's type script: every `AssetClass` but plain CKB, which is no type. */
export type CellClass = Exclude<AssetClass, 'ckb'>;

export type SealId =
  /** `txs`: how many transactions the block holds. */
  | { readonly kind: 'block'; readonly hash: string; readonly txs: number }
  /** `consumed` / `created`: its cells (a cellbase consumes 0); null where it is not known. */
  | {
      readonly kind: 'tx';
      readonly hash: string;
      readonly consumed: number | null;
      readonly created: number | null;
    }
  /**
   * `tx`, `index`: its out point. `lock`: its lock script's hash. `type`: its type script's class
   * and hash, null when it has none. `capacity`: in shannons.
   */
  | {
      readonly kind: 'cell';
      readonly tx: string;
      readonly index: number;
      readonly lock: string;
      readonly type: { readonly class: CellClass; readonly hash: string } | null;
      readonly capacity: bigint;
    }
  /** A script's hash; a lock script's is an address's. */
  | { readonly kind: 'script'; readonly hash: string };

export type SealKind = SealId['kind'];
export type SealForm = 'page' | 'row';
export type SealState = 'pressed' | 'ink' | 'unpressed';

export interface SealOptions {
  /** `page`: red text, the whole stone; `row`: white text cut out of a solid stone. */
  readonly form: SealForm;
  /** The seal's height in px. */
  readonly height: number;
  readonly state: SealState;
  /** The ink of a pressed seal (`color('mark')`). */
  readonly ink: string;
  /** The ground the seal is pressed on, the colour a row seal's text is cut in. */
  readonly ground: string;
  /** The ink of a seal in the `ink` state (`color('text-dim')`). */
  readonly dead: string;
  /** Degrees; the seal is turned with a CSS transform. */
  readonly tilt?: number;
  /** Pressed across a seam: the gap between two sheets shows through, this many px wide. */
  readonly seamPx?: number;
}

export interface DrawnSeal {
  readonly svg: string;
  /** px, to a tenth. */
  readonly width: number;
  readonly height: number;
}

/** A drawn seal and what it was drawn from, in the prototype's order (its `meta.chosen`). */
export interface Stone extends DrawnSeal {
  readonly chosen: readonly string[];
}

export type EncodingName = 'prototype' | 'compact';

/* ---------- the stone's measures (the prototype's defaults) ---------- */

const KINDS: readonly SealKind[] = ['block', 'tx', 'cell', 'script'];
const STATES: readonly SealState[] = ['pressed', 'ink', 'unpressed'];
const FORMS: readonly SealForm[] = ['page', 'row'];

/** The stroke weight over the lattice unit: a page seal's, a row seal's. */
const PAGE_RATIO = 0.58,
  ROW_RATIO = 0.7;
/** A page stone's frame width (in stroke weights) and its padding. */
const FRAMEW = 2.2,
  PAD = 1.0;
/** The glyph's box in units, on a page seal and on a row seal. */
const GB = 5.4,
  GBR = 6.2;
/** The written border's band, in pitches. */
const BAND = 1.55;
/** The hair line of an unpressed row stone. */
const HAIR = 0.6;
/** The brush of the rules between compartments. */
const RULE: BrushSpec = { curve: 'corner', r: 0.2, over: 0.1, entry: 'point', exit: 'taper' };
/** The compact encoding's tolerance, in px at the size drawn (as the talisman's row seal). */
export const ROW_TOLERANCE_PX = 0.2;
/**
 * How many digits a page seal writes a block's transaction count or a cell's index in. Neither
 * reaches five within CKB's 597,000-byte block: a transaction or an output takes far more than
 * 60 bytes.
 */
export const PAGE_DIGITS = 4;
/**
 * How many digits a transaction's page seal writes the cells it consumed or created in. An input
 * is 44 bytes (a 36-byte out point and an 8-byte since), so a 597,000-byte block holds a
 * transaction of up to 13,568 inputs: five digits, never six.
 */
export const TX_PAGE_DIGITS = 5;
/** How many decimal digits of whole CKB the beads of a cell can say. */
export const CAPACITY_DIGITS = 11;

/* ---------- the capacity ---------- */

/**
 * The number of decimal digits of a capacity in whole CKB (61 CKB: 2; 1,204: 4; 33.6 × 10⁹: 11),
 * from shannons, exactly. More than eleven digits is refused; `of` names the cell in the message.
 */
export function capacityDigits(shannons: bigint, of = 'a cell'): number {
  if (typeof shannons !== 'bigint' || shannons < BigInt(0))
    throw new Error(
      `seal: ${of}: a capacity is a whole number of shannons ≥ 0, got ${String(shannons)}`
    );
  const whole = shannons / SHANNONS_PER_CKB;
  const d = whole.toString().length;
  if (d > CAPACITY_DIGITS)
    throw new Error(
      `seal: ${of}: a capacity of ${shannons} shannons is ${whole} CKB, more than ${CAPACITY_DIGITS} digits`
    );
  return d;
}

/* ---------- checking an id ---------- */

const named = (id: SealId): string =>
  id.kind === 'cell'
    ? `cell ${String(id.tx)}:${String(id.index)}`
    : `${id.kind === 'tx' ? 'transaction' : id.kind} ${String(id.hash)}`;

function assertHash(of: string, field: string, value: unknown): void {
  if (typeof value !== 'string' || !SEAL_HASH.test(value))
    throw new Error(
      `seal: ${of}: ${field} is not a lowercase 32-byte hash: ${JSON.stringify(value)}`
    );
}

function assertCount(of: string, field: string, value: unknown): void {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < 0)
    throw new Error(`seal: ${of}: ${field} must be a whole number ≥ 0, got ${String(value)}`);
}

function assertPageDigits(of: string, what: string, n: number, max: number): void {
  if (decimalDigits(n).length > max)
    throw new Error(`seal: ${of}: a page seal writes ${what} in up to ${max} digits, got ${n}`);
}

function assertId(id: SealId, form: SealForm): void {
  if (typeof id !== 'object' || id === null || !KINDS.includes(id.kind))
    throw new Error(
      `seal: no such kind of seal: ${JSON.stringify((id as { kind?: unknown })?.kind)}`
    );
  const of = named(id);
  switch (id.kind) {
    case 'block':
      assertHash(of, 'the block hash', id.hash);
      assertCount(of, 'the transaction count', id.txs);
      if (form === 'page') assertPageDigits(of, 'the transaction count', id.txs, PAGE_DIGITS);
      return;
    case 'tx':
      assertHash(of, 'the transaction hash', id.hash);
      for (const [field, n] of [
        ['the cells consumed', id.consumed],
        ['the cells created', id.created],
      ] as const)
        if (n !== null) {
          assertCount(of, field, n);
          if (form === 'page') assertPageDigits(of, field, n, TX_PAGE_DIGITS);
        }
      return;
    case 'cell':
      assertHash(of, 'the out point’s transaction hash', id.tx);
      assertCount(of, 'the index', id.index);
      if (form === 'page') assertPageDigits(of, 'the index', id.index, PAGE_DIGITS);
      assertHash(of, 'the lock hash', id.lock);
      if (id.type !== null) {
        if (typeof id.type !== 'object')
          throw new Error(`seal: ${of}: the type must be { class, hash } or null`);
        if (!isAssetClass(id.type.class))
          throw new Error(`seal: ${of}: no such class ${JSON.stringify(id.type.class)}`);
        if ((id.type.class as AssetClass) === 'ckb')
          throw new Error(
            `seal: ${of}: class ckb is no type's class (a cell with no type script has type: null), got type hash ${JSON.stringify(id.type.hash)}`
          );
        if (id.type.hash === null || id.type.hash === undefined)
          throw new Error(`seal: ${of}: class ${id.type.class} with no type hash`);
        assertHash(of, 'the type hash', id.type.hash);
      }
      capacityDigits(id.capacity, of);
      return;
    case 'script':
      assertHash(of, 'the script hash', id.hash);
      return;
  }
}

function assertOptions(o: SealOptions): void {
  if (!FORMS.includes(o.form)) throw new Error(`seal: no such form ${JSON.stringify(o.form)}`);
  if (!STATES.includes(o.state)) throw new Error(`seal: no such state ${JSON.stringify(o.state)}`);
  if (!(typeof o.height === 'number' && Number.isFinite(o.height) && o.height > 0))
    throw new Error(`seal: height ${String(o.height)} px`);
  assertPaint('ink', o.ink);
  assertPaint('ground', o.ground);
  assertPaint('dead', o.dead);
  if (o.tilt !== undefined && !(typeof o.tilt === 'number' && Number.isFinite(o.tilt)))
    throw new Error(`seal: tilt ${String(o.tilt)}°`);
  if (
    o.seamPx !== undefined &&
    !(typeof o.seamPx === 'number' && Number.isFinite(o.seamPx) && o.seamPx >= 0)
  )
    throw new Error(`seal: seam ${String(o.seamPx)} px`);
}

/* ---------- the pen: one stone's hand in one encoding ---------- */

class Pen {
  readonly enc: Encoding;
  /** The textures already chosen, and the last (a texture never follows itself). */
  readonly chosen: string[] = [];
  private last: TextureName | null = null;

  constructor(
    readonly hand: Hand,
    H: number,
    height: number,
    encoding: EncodingName
  ) {
    this.enc =
      encoding === 'prototype' ? PROTOTYPE : compactEncoding(ROW_TOLERANCE_PX / (height / H));
  }

  draw(
    strokes: readonly FigureStroke[],
    brush: BrushSpec,
    X: (x: number) => number,
    Y: (y: number) => number,
    weight: number,
    paint: string,
    pitch?: number
  ): string {
    return this.enc.ink(outlinesOf(strokes, brush, X, Y, weight, this.hand, pitch), paint);
  }

  glyph(
    g: ChosenGlyph,
    bx: number,
    by: number,
    s: number,
    paint: string,
    weight: number,
    pitch?: number
  ) {
    return this.draw(
      g.strokes,
      CUT,
      (x) => bx + x * s,
      (y) => by + y * s,
      weight,
      paint,
      pitch
    );
  }

  /** A rim drawn as a line; a plain rim is a circle. */
  rimStroke(
    shape: RimName,
    cx: number,
    cy: number,
    R: number,
    paint: string,
    sw: number,
    dash = ''
  ) {
    return shape === 'round'
      ? `<circle cx="${f1(cx)}" cy="${f1(cy)}" r="${f2(R)}" fill="none" stroke="${paint}" stroke-width="${f2(sw)}"${dash}/>`
      : `<path d="${this.enc.ring(rimPoints(shape, cx, cy, R))}" fill="none" stroke="${paint}" stroke-width="${f2(sw)}" stroke-linejoin="round"${dash}/>`;
  }

  /** A rim filled. */
  rimFill(shape: RimName, cx: number, cy: number, R: number, paint: string) {
    return shape === 'round'
      ? `<circle cx="${f1(cx)}" cy="${f1(cy)}" r="${f2(R)}" fill="${paint}"/>`
      : `<path d="${this.enc.ring(rimPoints(shape, cx, cy, R))}" fill="${paint}"/>`;
  }

  /** A written texture in the box R (lattice units, margin m) of a field at (ox, oy). */
  texture(rd: BitReader, R: Box, ox: number, oy: number, m: number, paint: string): string {
    let idx = valueOf(rd.read(3));
    if (TEXTURES[idx] === this.last) idx = (idx + 1) % TEXTURES.length;
    const name = TEXTURES[idx];
    this.last = name;
    this.chosen.push(name);
    const pat = TEXTURE[name];
    const rw = Math.round(R.w - 2 * m),
      rh = Math.round(R.h - 2 * m);
    if (rw < 1 || rh < 1)
      throw new Error(`seal: a texture box of ${R.w} × ${R.h} units holds no lattice`);
    let strokes = pat.make(rd.read(pat.bits(rw, rh)).bits, rw, rh);
    if (!pat.brush.even) strokes = orderStrokes(strokes);
    const sx = (R.w - 2 * m) / rw,
      sy = (R.h - 2 * m) / rh;
    const p = this.hand.p;
    return this.draw(
      strokes,
      pat.brush,
      (x) => ox + (R.x + m + x * sx) * p,
      (yy) => oy + (R.y + m + yy * sy) * p,
      0.92,
      paint
    );
  }
}

/** What a stone function gives back: its size in units, its markup, what it chose. */
interface Drawn {
  readonly W: number;
  readonly H: number;
  readonly body: string;
  readonly chosen: readonly string[];
}

/** The paints and form of one drawing. */
interface Style {
  readonly row: boolean;
  readonly hand: Hand;
  /** The stone's ink: the dead ink in the `ink` state. */
  readonly col: string;
  /** The ink of the text: the ground cut out of a solid row stone, else the stone's ink. */
  readonly fc: string;
  readonly hollow: boolean;
  readonly ground: string;
  readonly dash: string;
  readonly height: number;
  readonly encoding: EncodingName;
}

/* ---------- the block ---------- */

/**
 * The block page's compartments beside the glyph's column: `nb` bands of heights 3 or 2 (one
 * bit each), filling the field with a gap between each two; and the rules between them.
 */
export function blockRegions(
  nb: number,
  heightBits: readonly number[],
  right: number
): { regions: Box[]; bars: FigureStroke[] } {
  const FH = 11.4,
    gap = 0.9,
    gw = GB + 0.2,
    ow = 11.4 - gw - gap;
  const tx0 = right ? 0 : gw + gap;
  const hs = heightBits.map((b, i) => [3, 2, 3, 2][(i + b) % 4]);
  if (hs.length !== nb) throw new Error(`seal: ${nb} bands with ${hs.length} heights`);
  const scale = (FH - (nb - 1) * gap) / hs.reduce((a, c) => a + c, 0);
  const regions: Box[] = [],
    bars: FigureStroke[] = [];
  let y = 0;
  for (let i = 0; i < nb; i++) {
    const h = hs[i] * scale;
    regions.push({ x: tx0, y, w: ow, h });
    y += h;
    if (i < nb - 1) {
      bars.push({
        pts: [
          [tx0 + 0.3, y + gap / 2],
          [tx0 + ow - 0.3, y + gap / 2],
        ],
        horiz: true,
      });
      y += gap;
    }
  }
  return { regions, bars };
}

function blockStone(id: Extract<SealId, { kind: 'block' }>, st: Style): Drawn {
  const { row, hand, col, fc, hollow, dash } = st;
  const p = hand.p,
    w0 = hand.w0;
  const rd = new BitReader(id.hash),
    g = glyphOf(rd);
  const chosen: string[] = [g.name];
  let body = '';
  if (row) {
    /* the glyph, and under a line the count in rods (to nine; a full block beyond) */
    const s = 5.2,
      side = 8.8;
    const pen = new Pen(hand, side, st.height, st.encoding);
    body += hollow
      ? `<rect x="${HAIR / 2}" y="${HAIR / 2}" width="${f1(side - HAIR)}" height="${f1(side - HAIR)}" rx="0.6" fill="none" stroke="${col}" stroke-width="${HAIR}"${dash}/>`
      : `<rect width="${f1(side)}" height="${f1(side)}" rx="0.6" fill="${col}"/>`;
    body += pen.glyph(g, (side - s) / 2, 0.5, s, fc, 1.15, p * 0.9);
    body += `<path d="M0.9 6.15L${f1(side - 0.9)} 6.15" stroke="${fc}" stroke-width="0.32"/>`;
    const r = rodCount(
      id.txs,
      { x: 0.9, y: 6.55, w: side - 1.8, h: 1.75 },
      { pitch: 0.95, maxDigits: 1, digitH: 1.75 }
    );
    body += r.full
      ? `<rect x="${f2(side / 2 - 2.2)}" y="6.65" width="4.4" height="1.55" fill="${fc}"/>`
      : pen.draw(
          r.strokes,
          ROD,
          (xx) => xx,
          (yy) => yy,
          0.85,
          fc,
          p * 0.6
        );
    return { W: side, H: side, body, chosen };
  }
  const FW = 11.4,
    FH = 11.4,
    gap = 0.9;
  const wf = w0 * FRAMEW,
    pad = PAD;
  const W = 2 * wf + 2 * pad + FW * p,
    H = W;
  const pen = new Pen(hand, H, st.height, st.encoding);
  const ox = wf + pad,
    oy = wf + pad;
  const gw = GB + 0.2;
  const right = valueOf(rd.read(1)),
    top = valueOf(rd.read(1));
  const gx = right ? FW - gw : 0;
  const gy = top ? 0 : FH - GB;
  let field = pen.glyph(g, ox + (gx + 0.1) * p, oy + gy * p, GB * p, col, 1.3, p * 1.25);
  /* beside the glyph, in its own column: how many transactions, digit under digit */
  const cz = { x: gx, y: top ? GB + gap : 0, w: gw, h: FH - GB - gap };
  const nd = decimalDigits(id.txs).length;
  const r = rodCount(
    id.txs,
    { x: ox + cz.x * p, y: oy + (cz.y + 0.35) * p, w: cz.w * p, h: (cz.h - 0.7) * p },
    { pitch: 0.66 * p, maxDigits: PAGE_DIGITS, digitH: [4.4, 2.9, 1.95, 1.4][nd - 1] * p }
  );
  if (r.full) throw new Error(`seal: ${named(id)}: ${id.txs} transactions do not fit four digits`);
  field += pen.draw(
    r.strokes,
    ROD,
    (xx) => xx,
    (yy) => yy,
    1.05,
    col,
    p
  );
  const nb = 2 + valueOf(rd.read(1));
  const heightBits: number[] = [];
  for (let i = 0; i < nb; i++) heightBits.push(valueOf(rd.read(1)));
  const { regions, bars } = blockRegions(nb, heightBits, right);
  bars.push({
    pts: [
      [gx + 0.3, top ? GB + gap / 2 : FH - GB - gap / 2],
      [gx + gw - 0.3, top ? GB + gap / 2 : FH - GB - gap / 2],
    ],
    horiz: true,
  });
  const vxr = right ? gx - gap / 2 : gw + gap / 2;
  bars.push({
    pts: [
      [vxr, 0.1],
      [vxr, FH - 0.1],
    ],
    horiz: false,
    weight: 0.8,
  });
  for (const R of regions) field += pen.texture(rd, R, ox, oy, 0.25, col);
  field += pen.draw(
    bars,
    RULE,
    (x) => ox + x * p,
    (yy) => oy + yy * p,
    0.85,
    col
  );
  if (hollow) field = `<g opacity="0.5">${field}</g>`;
  body +=
    `<rect x="${f1(wf / 2)}" y="${f1(wf / 2)}" width="${f1(W - wf)}" height="${f1(H - wf)}" rx="0.7" fill="none" stroke="${col}" stroke-width="${f1(hollow ? wf * 0.4 : wf)}"${hollow ? dash : ''}/>` +
    field;
  return { W, H, body, chosen: [...chosen, ...pen.chosen] };
}

/* ---------- the script ---------- */

/** The script page's texture box: under or over the glyph, the stone's width, 3.2 units tall. */
export const SCRIPT_TEXTURE = { w: GB + 0.4, h: 3.2, margin: 0.2 } as const;

function scriptStone(id: Extract<SealId, { kind: 'script' }>, st: Style): Drawn {
  const { row, hand, col, fc, hollow, dash } = st;
  const p = hand.p,
    w0 = hand.w0;
  const rd = new BitReader(id.hash),
    g = glyphOf(rd);
  const chosen: string[] = [g.name];
  let body = '';
  if (row) {
    const s = GBR,
      bw = s + 1.6,
      bh = s + 4.2;
    const pen = new Pen(hand, bh, st.height, st.encoding);
    body += hollow
      ? `<rect x="${HAIR / 2}" y="${HAIR / 2}" width="${f1(bw - HAIR)}" height="${f1(bh - HAIR)}" rx="${f1(bw * 0.36)}" fill="none" stroke="${col}" stroke-width="${HAIR}"${dash}/>`
      : `<rect width="${f1(bw)}" height="${f1(bh)}" rx="${f1(bw * 0.36)}" fill="${col}"/>`;
    body += pen.glyph(g, 0.8, (bh - s) / 2, s, fc, 1.2, p);
    return { W: bw, H: bh, body, chosen };
  }
  const FW = SCRIPT_TEXTURE.w,
    gap = 0.9,
    th = SCRIPT_TEXTURE.h,
    FH = GB + gap + th;
  const wf = w0 * 1.3,
    pad = 1.7;
  const W = 2 * wf + 2 * pad + FW * p,
    H = 2 * wf + 2 * pad + FH * p;
  const pen = new Pen(hand, H, st.height, st.encoding);
  const ox = wf + pad,
    oy = wf + pad,
    top = valueOf(rd.read(1));
  /* eight bits the prototype passes over here ("the owner's hour, read by its cells") */
  rd.read(8);
  let field = pen.glyph(g, ox + 0.2 * p, oy + (top ? 0 : th + gap) * p, GB * p, col, 1.3, p * 1.25);
  field += pen.texture(
    rd,
    { x: 0, y: top ? GB + gap : 0, w: FW, h: th },
    ox,
    oy,
    SCRIPT_TEXTURE.margin,
    col
  );
  field += pen.draw(
    [
      {
        pts: [
          [0.3, top ? GB + gap / 2 : th + gap / 2],
          [FW - 0.3, top ? GB + gap / 2 : th + gap / 2],
        ],
        horiz: true,
      },
    ],
    RULE,
    (x) => ox + x * p,
    (yy) => oy + yy * p,
    0.85,
    col
  );
  if (hollow) field = `<g opacity="0.5">${field}</g>`;
  body +=
    `<rect x="${f1(wf / 2)}" y="${f1(wf / 2)}" width="${f1(W - wf)}" height="${f1(H - wf)}" rx="${f1(W * 0.34)}" fill="none" stroke="${col}" stroke-width="${f1(hollow ? wf * 0.4 : wf)}"${hollow ? dash : ''}/>` +
    field;
  return { W, H, body, chosen: [...chosen, ...pen.chosen] };
}

/* ---------- the transaction ---------- */

/** A transaction's page stone in its units (the prototype's measures, from the hand). */
export interface TxPageMeasures {
  readonly p: number;
  readonly w0: number;
  /** The glyph's side. */
  readonly s: number;
  /** The radius inside the written border, and the border's band. */
  readonly rIn: number;
  readonly band: number;
  /** The rim's width, and its outer radius at its widest. */
  readonly rimw: number;
  readonly Rout: number;
  /** The frame's width. */
  readonly wf: number;
  readonly W: number;
  readonly H: number;
  /** The core's centre. */
  readonly cx: number;
  readonly cy: number;
}

export function txPageMeasures(hand: Hand): TxPageMeasures {
  const { p, w0 } = hand;
  const s = GB * p,
    rIn = (s / 2) * Math.SQRT2 * 0.88,
    band = BAND * p;
  const rimw = w0 * 1.3,
    Rout = (rIn + band + 0.15 * p + rimw / 2) / RIM_MIN;
  const wf = w0 * FRAMEW,
    fgap = 0.2 * p,
    zw = 4.3 * p;
  const H = 2 * (Rout + fgap + wf),
    W = H + 2 * zw;
  return { p, w0, s, rIn, band, rimw, Rout, wf, W, H, cx: W / 2, cy: H / 2 };
}

/**
 * A digit's height in a transaction page's count, in pitches, by how many digits the count has
 * (the prototype's one to four; the fifth is the port's, the stack about as tall as three's and
 * four's).
 */
export const TX_DIGIT_H = [4.2, 3.0, 2.05, 1.5, 1.2] as const;

/** A count of cells written at one end of a transaction's page seal. */
export interface TxCount {
  /** The zone the count stands in, between the frame and the rim. */
  readonly zone: Box;
  /** The distance between two rods. */
  readonly pitch: number;
  readonly strokes: FigureStroke[];
}

/**
 * The cells a transaction consumed (`end` 0, at the left) or created (1, at the right), in rods on
 * its page seal: digit under digit, the greatest on top, centred in the zone between the frame and
 * the rim; up to five digits (`TX_PAGE_DIGITS`), more is refused.
 */
export function txCountRods(n: number, end: 0 | 1, m: TxPageMeasures): TxCount {
  const { p, wf, cx, Rout, H } = m;
  const nd = decimalDigits(n).length;
  if (nd > TX_PAGE_DIGITS)
    throw new Error(
      `seal: a transaction's page seal writes its cells in up to ${TX_PAGE_DIGITS} digits, got ${n}`
    );
  const zx = end ? cx + Rout + 0.15 * p : wf + 0.5 * p,
    zwi = cx - Rout - wf - 0.65 * p;
  const zone: Box = { x: zx, y: wf + 0.9 * p, w: zwi, h: H - 2 * wf - 1.8 * p };
  const pitch = 0.66 * p;
  const r = rodCount(n, zone, {
    pitch,
    maxDigits: TX_PAGE_DIGITS,
    digitH: TX_DIGIT_H[nd - 1] * p,
  });
  if (r.full) throw new Error(`seal: ${n} cells do not fit ${TX_PAGE_DIGITS} digits`);
  return { zone, pitch, strokes: r.strokes };
}

function txStone(id: Extract<SealId, { kind: 'tx' }>, st: Style): Drawn {
  const { row, hand, col, fc, hollow, dash } = st;
  const p = hand.p,
    w0 = hand.w0;
  const core = coreOf(id.hash),
    g = core.glyph;
  const chosen = [g.name, core.ring, core.rim];
  const counts = [id.consumed, id.created];
  let body = '';
  if (row) {
    const s = GBR,
      rc = ((s / 2) * Math.SQRT2 * 0.9 + 0.3) / RIM_MIN,
      bh = 2 * (rc + 0.55),
      zw = 5.3,
      bw = bh + 2 * zw;
    const pen = new Pen(hand, bh, st.height, st.encoding);
    body += hollow
      ? `<rect x="${HAIR / 2}" y="${HAIR / 2}" width="${f1(bw - HAIR)}" height="${f1(bh - HAIR)}" rx="0.6" fill="none" stroke="${col}" stroke-width="${HAIR}"${dash}/>`
      : `<rect width="${f1(bw)}" height="${f1(bh)}" rx="0.6" fill="${col}"/>`;
    body += pen.rimStroke(core.rim, bw / 2, bh / 2, rc, fc, 0.5);
    body += pen.glyph(g, bw / 2 - s / 2, bh / 2 - s / 2, s, fc, 1.2, p);
    counts.forEach((n, i) => {
      if (n === null)
        return; /* not known (a transaction the store does not hold): the end stays empty */
      const x = i ? bw / 2 + rc + 0.25 : 0.55,
        zwi = bw / 2 - rc - 0.8;
      const r = rodCount(
        n,
        { x, y: 1.5, w: zwi, h: bh - 3 },
        { pitch: 0.98, maxDigits: 1, digitH: 5.6 }
      );
      if (r.full)
        body += `<rect x="${f2(x + zwi * 0.2)}" y="${f2(bh * 0.24)}" width="${f2(zwi * 0.6)}" height="${f2(bh * 0.52)}" fill="${fc}"/>`;
      else
        body += pen.draw(
          r.strokes,
          ROD,
          (xx) => xx,
          (yy) => yy,
          0.95,
          fc,
          p * 0.8
        );
    });
    return { W: bw, H: bh, body, chosen };
  }
  const m = txPageMeasures(hand);
  const { s, rIn, band, rimw, Rout, wf, W, H, cx, cy } = m;
  const pen = new Pen(hand, H, st.height, st.encoding);
  const Rm = rIn + band / 2 + 0.05 * p;
  let coreSvg = pen.draw(
    ringStrokes(core.ring, Rm, band * 0.3, core.k, cx, cy, core.flip),
    CUT,
    (x) => x,
    (y) => y,
    0.8,
    col,
    p
  );
  coreSvg += `<circle cx="${f1(cx)}" cy="${f1(cy)}" r="${f1(rIn + 0.1 * p)}" fill="none" stroke="${col}" stroke-width="${f1(w0 * 0.32)}"/>`;
  coreSvg += pen.glyph(g, cx - s / 2, cy - s / 2, s, col, 1.3, p * 1.25);
  counts.forEach((n, i) => {
    if (n === null) return;
    coreSvg += pen.draw(
      txCountRods(n, i ? 1 : 0, m).strokes,
      ROD,
      (xx) => xx,
      (yy) => yy,
      1.05,
      col,
      p
    );
  });
  if (hollow) coreSvg = `<g opacity="0.5">${coreSvg}</g>`;
  body += `<rect x="${f1(wf / 2)}" y="${f1(wf / 2)}" width="${f1(W - wf)}" height="${f1(H - wf)}" rx="0.7" fill="none" stroke="${col}" stroke-width="${f1(hollow ? wf * 0.4 : wf)}"${hollow ? dash : ''}/>`;
  body +=
    pen.rimStroke(core.rim, cx, cy, Rout - rimw / 2, col, hollow ? rimw * 0.4 : rimw) + coreSvg;
  return { W, H, body, chosen };
}

/* ---------- the cell ---------- */

/** Where a cell's page seal writes its index: its digits' boxes, left to right. */
export interface IndexLayout {
  /** The distance between two rods. */
  readonly pitch: number;
  /** The digits' width with the gaps between them: the border opens this wide at the foot. */
  readonly total: number;
  readonly digits: readonly { readonly d: number; readonly box: Box }[];
}

/**
 * The index of a cell, in rods along the foot of its border (centre `c`, the border's middle
 * radius `RmB`, its band `bb`, pitch `p`): each digit as wide as its rods, the digits a little
 * apart, centred under the stone, each standing on the border's circle. Up to four digits.
 */
export function indexLayout(
  index: number,
  c: number,
  RmB: number,
  bb: number,
  p: number
): IndexLayout {
  const ds = decimalDigits(index),
    pitch = 0.46 * p,
    dh = bb * 0.8;
  if (ds.length > PAGE_DIGITS)
    throw new Error(
      `seal: a page seal writes an index in up to ${PAGE_DIGITS} digits, got ${index}`
    );
  const widths = ds.map((d) => (d === 0 ? 1.1 : d > 5 ? Math.max(d - 5, 2) : d) * pitch + 0.1 * p),
    dgap = 0.55 * p;
  const total = widths.reduce((a2, b2) => a2 + b2, 0) + (ds.length - 1) * dgap;
  const digits: { d: number; box: Box }[] = [];
  let xx = c - total / 2;
  ds.forEach((d, i) => {
    const xc = xx + widths[i] / 2,
      under = RmB * RmB - (xc - c) * (xc - c);
    if (under < 0) throw new Error(`seal: index ${index} runs off the border`);
    const dy = Math.sqrt(under);
    digits.push({ d, box: { x: xx, y: c + dy - dh / 2, w: widths[i], h: dh } });
    xx += widths[i] + dgap;
  });
  return { pitch, total, digits };
}

function cellStone(id: Extract<SealId, { kind: 'cell' }>, st: Style): Drawn {
  const { row, hand, col, fc, hollow, dash, ground } = st;
  const p = hand.p,
    w0 = hand.w0;
  const core = coreOf(id.tx),
    own = ownerOf(id.lock),
    nd = capacityDigits(id.capacity, named(id));
  const tg = id.type ? glyphOf(new BitReader(id.type.hash)) : null;
  const chosen = [tg ? tg.name : '-', core.rim, core.ring, own.glyph.name];
  const ha = hourAngle(own.hour);
  let body = '';
  if (row) {
    const s = 6.0,
      rw = (s / 2) * Math.SQRT2 * 1.02,
      band = 1.15,
      Rm = rw + 0.28 + band / 2,
      R = (rw + 0.28 + band + 0.2) / RIM_MIN;
    const W = 2 * R,
      H = W;
    const pen = new Pen(hand, H, st.height, st.encoding);
    const c = R;
    body += hollow
      ? pen.rimStroke(core.rim, c, c, R - HAIR / 2, col, HAIR, dash)
      : pen.rimFill(core.rim, c, c, R, col);
    for (let j = 1; j <= nd; j++) {
      const a = hourAngle(own.hour + j);
      body += `<circle cx="${f2(c + Rm * Math.cos(a))}" cy="${f2(c + Rm * Math.sin(a))}" r="0.4" fill="${fc}"/>`;
    }
    /* the owner's stone, as small as it goes: an upright mark at the owner's hour */
    body += `<rect x="${f2(c + Rm * Math.cos(ha) - 0.52)}" y="${f2(c + Rm * Math.sin(ha) - 0.74)}" width="1.04" height="1.48" rx="0.36" fill="${fc}"/>`;
    if (id.type && tg) {
      body += windowOf(id.type.class, c, c, rw + 0.1, fc, 0.42);
      body += pen.glyph(
        tg,
        c - s / 2,
        c - s / 2 + (id.type.class === 'identities' ? 0.25 : 0),
        s,
        fc,
        1.15,
        p * 0.9
      );
    }
    return { W, H, body, chosen };
  }
  const s = GB * p * 0.92,
    rw = (s / 2) * Math.SQRT2 * 1.02,
    bb = 1.4 * p;
  const RmB = rw + 0.25 * p + bb / 2;
  /* the rim's centre line, at its widest */
  const rimw = w0 * 1.4,
    Rr = (RmB + bb / 2 + 0.2 * p + rimw / 2) / RIM_MIN;
  /* room for the beads and for the owner's stone where it overhangs the rim */
  const m = 1.25 * p;
  const W = 2 * (Rr + rimw / 2 + m),
    H = W;
  const pen = new Pen(hand, H, st.height, st.encoding);
  const c = W / 2;
  /* the index, in rods, where the border opens at the foot */
  const ix = indexLayout(id.index, c, RmB, bb, p);
  const total = ix.total;
  let idx = '';
  for (const { d, box } of ix.digits)
    idx += pen.draw(
      rodDigit(d, box, ix.pitch),
      ROD,
      (x) => x,
      (y) => y,
      0.95,
      col,
      p * 0.8
    );
  /* the owner's stone, on the border at the owner's hour */
  const cw = 2.9 * p,
    ch = 3.9 * p,
    qr = RmB + 0.45 * p,
    qx = c + qr * Math.cos(ha),
    qy = c + qr * Math.sin(ha);
  const gaps: Gap[] = [
    [Math.PI / 2, (total / 2 + 0.55 * p) / RmB],
    [ha, (cw * 0.7) / RmB],
  ];
  let coreSvg = pen.draw(
    cutGaps(ringStrokes(core.ring, RmB, bb * 0.3, core.k, c, c, core.flip), c, c, gaps),
    CUT,
    (x) => x,
    (y) => y,
    0.8,
    col,
    p
  );
  coreSvg += idx;
  if (id.type && tg) {
    coreSvg += windowOf(id.type.class, c, c, rw + 0.05 * p, col, w0 * 0.4);
    coreSvg += pen.glyph(
      tg,
      c - s / 2,
      c - s / 2 + (id.type.class === 'identities' ? 0.04 * s : 0),
      s,
      col,
      1.3,
      p * 1.25
    );
  }
  if (hollow) coreSvg = `<g opacity="0.5">${coreSvg}</g>`;
  body +=
    pen.rimStroke(core.rim, c, c, Rr, col, hollow ? rimw * 0.4 : rimw, hollow ? dash : '') +
    coreSvg;
  /* the capacity: a bead on the rim for each digit, clockwise from the owner's stone */
  for (let j = 1; j <= nd; j++) {
    const h = own.hour + j,
      a = hourAngle(h),
      rr = Rr * rimR(core.rim, (h * TAU) / 12);
    body += `<circle cx="${f2(c + rr * Math.cos(a))}" cy="${f2(c + rr * Math.sin(a))}" r="${f2(0.5 * p)}" fill="${hollow ? ground : col}" stroke="${hollow ? col : ground}" stroke-width="${f2(hollow ? 0.3 : 0.16 * p)}" paint-order="stroke"/>`;
  }
  const gs = cw * 0.8;
  body += `<rect x="${f2(qx - cw / 2)}" y="${f2(qy - ch / 2)}" width="${f2(cw)}" height="${f2(ch)}" rx="${f2(cw * 0.36)}" fill="${hollow ? ground : col}" stroke="${hollow ? col : ground}" stroke-width="${f2(hollow ? 0.35 : 0.3 * p)}"${hollow ? dash : ''} paint-order="stroke"/>`;
  body += pen.glyph(own.glyph, qx - gs / 2, qy - gs / 2, gs, hollow ? col : ground, 0.66, p * 0.55);
  return { W, H, body, chosen };
}

/* ---------- the label ---------- */

const counted = (n: number | null, what: string) =>
  n === null ? `${what} not known` : `${n} ${what}`;

/** What the seal says to a screen reader: the kind, the id, and what it counts. */
export function sealLabel(id: SealId): string {
  switch (id.kind) {
    case 'block':
      return `Block seal ${id.hash.slice(0, 10)}, ${id.txs} transaction${id.txs === 1 ? '' : 's'}`;
    case 'tx':
      return `Transaction seal ${id.hash.slice(0, 10)}, ${counted(id.consumed, 'consumed')}, ${counted(id.created, 'created')}`;
    case 'cell':
      return `Cell seal ${id.tx.slice(0, 10)}:${id.index}, ${id.type ? id.type.class : 'ckb'}, ${exactCkbText(id.capacity, false)} CKB`;
    case 'script':
      return `Script seal ${id.hash.slice(0, 10)}`;
  }
}

/* ---------- the entries ---------- */

/** A form's hand: its stroke weight over the lattice unit, and the pitch. */
export function handOf(form: SealForm): Hand {
  const w0 = form === 'row' ? ROW_RATIO : PAGE_RATIO;
  return { w0, p: 1 + w0 };
}

/**
 * Refuses an id that cannot be drawn in `form`, naming it: every check `drawSeal` makes of an id.
 * For a component that draws once per id but checks the id it is given on every render.
 */
export function assertSealId(id: SealId, form: SealForm): void {
  if (!FORMS.includes(form)) throw new Error(`seal: no such form ${JSON.stringify(form)}`);
  assertId(id, form);
}

/**
 * The stone of `id` and what it was drawn from, in the given encoding (a page seal's is the
 * prototype's, a row seal's the compact one). `drawSeal` is the entry for pages and components.
 */
export function drawStone(id: SealId, o: SealOptions, encoding?: EncodingName): Stone {
  assertOptions(o);
  assertId(id, o.form);
  const row = o.form === 'row';
  const hand = handOf(o.form);
  const col = o.state === 'ink' ? o.dead : o.ink;
  const hollow = o.state === 'unpressed';
  const st: Style = {
    row,
    hand,
    col,
    fc: row && !hollow ? o.ground : col,
    hollow,
    ground: o.ground,
    dash: ` stroke-dasharray="${row ? '1.5 1.1' : '1.9 1.4'}"`,
    height: o.height,
    encoding: encoding ?? (row ? 'compact' : 'prototype'),
  };
  const drawn =
    id.kind === 'block'
      ? blockStone(id, st)
      : id.kind === 'tx'
        ? txStone(id, st)
        : id.kind === 'cell'
          ? cellStone(id, st)
          : scriptStone(id, st);
  const { W, H } = drawn;
  let body = drawn.body;
  const scale = o.height / H;
  /* pressed across a seam: the gap between the two sheets shows through, seamPx wide */
  if (o.seamPx)
    body += `<path d="M${f2(W / 2)} -1L${f2(W / 2)} ${f1(H + 1)}" stroke="${o.ground}" stroke-width="${f2(o.seamPx / scale)}"/>`;
  const width = Math.round(W * scale * 10) / 10,
    height = Math.round(H * scale * 10) / 10;
  const tilt = o.tilt ?? 0;
  const svg = `<svg width="${width}" height="${height}" viewBox="${f1(0)} ${f1(0)} ${f1(W)} ${f1(H)}" role="img" aria-label="${sealLabel(id)}"${tilt ? ` style="transform: rotate(${tilt}deg)"` : ''}>${body}</svg>`;
  return { svg, width, height, chosen: drawn.chosen };
}

/** The seal of `id`: its markup and its size in px. */
export function drawSeal(id: SealId, o: SealOptions): DrawnSeal {
  const { svg, width, height } = drawStone(id, o);
  return { svg, width, height };
}
