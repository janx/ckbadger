/**
 * Geometry and words of the to-scale byte maps in tier 2 (consistency lane E, rulings C5 / C6): the
 * cell-data map of a protocol cell, the lock or type script's 53 bytes, a DOB pattern's DNA ruler.
 * Everything here reads the bytes and ranges it is given; a range outside its bytes throws, and the
 * words name only what the bytes show (a length that matches, text that decodes), never a guess.
 */

/** One range of a byte map: `[start, end)`; `end === start` is an empty field (a sliver). */
export interface ByteSpan {
  start: number;
  end: number;
}
/** A described range of the bytes, by id. */
export interface IdSpan extends ByteSpan {
  id: string;
}
/** A maximal run of bytes read by the same fields; `ids` empty = bytes no field describes. */
export interface BytePiece extends ByteSpan {
  ids: string[];
}

function checkSpan(span: IdSpan, total: number, context: string) {
  if (
    !Number.isSafeInteger(span.start) ||
    !Number.isSafeInteger(span.end) ||
    span.start < 0 ||
    span.end < span.start ||
    span.end > total
  )
    throw new Error(
      `${context}: byte range ${span.id} [${span.start}, ${span.end}) outside ${total} bytes`
    );
}

/**
 * The bytes `[0, total)` cut wherever a field starts or ends: each piece lists the fields that read
 * it (in input order), a piece no field reads has none, and an empty field is a zero-length piece
 * at its offset. The pieces cover every byte exactly once.
 */
export function bytePieces(total: number, spans: readonly IdSpan[], context: string): BytePiece[] {
  if (!Number.isSafeInteger(total) || total < 0)
    throw new Error(`${context}: invalid byte length ${total}`);
  for (const span of spans) checkSpan(span, total, context);
  const edges = [...new Set([0, total, ...spans.flatMap((s) => [s.start, s.end])])].sort(
    (a, b) => a - b
  );
  const runs: BytePiece[] = [];
  for (let i = 0; i + 1 < edges.length; i++) {
    const [start, end] = [edges[i], edges[i + 1]];
    const ids = spans
      .filter((s) => s.end > s.start && s.start <= start && s.end >= end)
      .map((s) => s.id);
    const last = runs.at(-1);
    if (last && last.ids.join('\u0000') === ids.join('\u0000')) last.end = end;
    else runs.push({ start, end, ids });
  }
  const pieces: BytePiece[] = [];
  const empties = spans.filter((s) => s.end === s.start);
  for (const run of runs) {
    for (const e of empties)
      if (e.start === run.start) pieces.push({ start: e.start, end: e.start, ids: [e.id] });
    pieces.push(run);
  }
  for (const e of empties)
    if (e.start === total) pieces.push({ start: total, end: total, ids: [e.id] });
  return pieces;
}

export interface ScaleOptions {
  /** Pixels between two parts. */
  gap: number;
  /** The narrowest a non-empty part is drawn. */
  floor: number;
  /** The width of an empty part. */
  sliver: number;
}
export const SCALE_DEFAULTS: ScaleOptions = { gap: 2, floor: 3, sliver: 4 };

/**
 * Lay parts out across `width` px: empty parts as fixed slivers, every other part in proportion to
 * its bytes but never narrower than its `min` (room for its label; the floor at least) — a part
 * below it is fixed there and the rest is shared again, as the cell page's capacity map does. Returns each part's left edge and width; the last part ends at `width`.
 */
export function scaleLayout(
  width: number,
  spans: readonly (ByteSpan & { min?: number })[],
  options: Partial<ScaleOptions> = {}
): { x: number; w: number }[] {
  const { gap, floor, sliver } = { ...SCALE_DEFAULTS, ...options };
  if (!(width > 0)) throw new Error(`byte map: invalid width ${width}`);
  const n = spans.length;
  if (n === 0) return [];
  const sized = spans.map((s, i) => ({ i, bytes: s.end - s.start })).filter((s) => s.bytes > 0);
  const available = width - gap * (n - 1) - sliver * (n - sized.length);
  // Each part's own minimum (room for its byte count) while they all fit; else the floor; a phone
  // too narrow even for the floors shares what there is (layout only: every part is still drawn).
  const own = sized.map((s) => Math.max(floor, spans[s.i].min ?? floor));
  const mins = new Map(
    sized.map((s, k) => [
      s.i,
      own.reduce((a, b) => a + b, 0) <= available
        ? own[k]
        : Math.min(floor, Math.max(0, available) / sized.length),
    ])
  );
  const fixed = new Set<number>();
  let share = new Map<number, number>();
  for (;;) {
    const free = sized.filter((s) => !fixed.has(s.i));
    const room = available - [...fixed].reduce((sum, i) => sum + mins.get(i)!, 0);
    const bytes = free.reduce((sum, s) => sum + s.bytes, 0);
    share = new Map(free.map((s) => [s.i, (room * s.bytes) / bytes]));
    const below = free.filter((s) => share.get(s.i)! < mins.get(s.i)!);
    if (!below.length) break;
    below.forEach((s) => fixed.add(s.i));
  }
  let x = 0;
  return spans.map((s, i) => {
    const w = s.end === s.start ? sliver : fixed.has(i) ? mins.get(i)! : share.get(i)!;
    const part = { x, w };
    x += w + gap;
    return part;
  });
}

/** The advance of one 11 px JetBrains Mono glyph (`text-meta`). */
export const META_CHAR_PX = 6.6;

export interface RulerTick {
  value: number;
  x: number;
  /** Empty when the label has no room. */
  label: string;
  anchor: 'start' | 'end';
}

/**
 * The offsets ruler under a map: a tick at every value in `values` (part boundaries, ascending; the
 * last is the total, at the right edge). Labels go left to right where they fit, 4 px apart; the
 * first and the last always carry theirs, the last right-aligned.
 */
export function scaleRulerTicks(
  layout: readonly { x: number; w: number }[],
  values: readonly number[],
  width: number,
  format: (n: number) => string
): RulerTick[] {
  if (layout.length + 1 !== values.length)
    throw new Error(`byte map ruler: ${values.length} offsets for ${layout.length} parts`);
  const text = (v: number) => format(v).length * META_CHAR_PX;
  const lastLeft = width - text(values.at(-1)!);
  let right = -Infinity;
  return values.map((value, i) => {
    const last = i === values.length - 1;
    const x = last ? width : layout[i].x;
    const fits = i === 0 || last || (x >= right + 4 && x + text(value) + 4 <= lastLeft);
    if (fits && !last) right = x + text(value);
    return { value, x, label: fits ? format(value) : '', anchor: last ? 'end' : 'start' };
  });
}

const u32le = (bytes: readonly string[], at: number) =>
  Number.parseInt(
    bytes
      .slice(at, at + 4)
      .reverse()
      .join(''),
    16
  );

function printable(bytes: readonly string[]): string | null {
  try {
    const decoded = new TextDecoder('utf-8', { fatal: true }).decode(
      Uint8Array.from(bytes, (b) => Number.parseInt(b, 16))
    );
    return /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/.test(decoded) ? null : decoded;
  } catch {
    return null;
  }
}

function contentWords(bytes: readonly string[]): string {
  const n = `${bytes.length.toLocaleString('en-US')} ${bytes.length === 1 ? 'byte' : 'bytes'}`;
  const decoded = bytes.length ? printable(bytes) : null;
  if (decoded === null) return n;
  if (/^\s*[[{]/.test(decoded)) {
    try {
      JSON.parse(decoded);
      return `${n} of JSON`;
    } catch {
      // text that only starts like JSON is text
    }
  }
  return `${n} of UTF-8 text`;
}

/**
 * What a field's bytes are, from the bytes alone: empty; a molecule `Bytes` (a little-endian
 * 4-byte length that matches the rest) with what follows it; UTF-8 text or JSON that decodes
 * exactly; otherwise only how many bytes there are.
 */
export function fieldShape(bytes: readonly string[]): string {
  if (bytes.length === 0) return 'empty: no bytes';
  if (bytes.length >= 4 && u32le(bytes, 0) === bytes.length - 4) {
    const rest = bytes.slice(4);
    return `a 4-byte length (${rest.length.toLocaleString('en-US')}), then ${rest.length ? contentWords(rest) : 'nothing'}`;
  }
  return contentWords(bytes);
}

/**
 * A molecule table header at the start of the bytes, `[0, size)`: the first u32 is the size of all
 * the bytes, the second the header's own size, and the offsets that follow are exactly `starts`
 * (the fields after it). Null when any of that does not hold.
 */
export function moleculeTableHeader(
  bytes: readonly string[],
  size: number,
  starts: readonly number[]
): { total: number; offsets: number[] } | null {
  if (size < 8 || size % 4 !== 0 || size > bytes.length) return null;
  if (u32le(bytes, 0) !== bytes.length || u32le(bytes, 4) !== size) return null;
  const offsets = Array.from({ length: size / 4 - 1 }, (_, i) => u32le(bytes, 4 + 4 * i));
  if (offsets.length !== starts.length || offsets.some((o, i) => o !== starts[i])) return null;
  return { total: bytes.length, offsets };
}

/** Bytes as 4-byte words (`0b000000 2e63656c …`), the way the cell page's readouts group them. */
export function hexWords(bytes: readonly string[]): string[] {
  const words: string[] = [];
  for (let i = 0; i < bytes.length; i += 4) words.push(bytes.slice(i, i + 4).join(''));
  return words;
}

/** Printable ASCII of the bytes, a dot for every other byte. */
export function asciiPreview(bytes: readonly string[]): string {
  return bytes
    .map((b) => {
      const code = Number.parseInt(b, 16);
      return code >= 32 && code <= 126 ? String.fromCharCode(code) : '.';
    })
    .join('');
}
