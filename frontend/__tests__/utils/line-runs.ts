import { foreignGraphemes, MONO_ADVANCE_EM } from '@/lib/type-metrics';
import {
  boxPx,
  breaksWordsAt,
  displayAt,
  flexColumnAt,
  flexWrapsAt,
  minWidthAt,
  gapPx,
  inFlowAt,
  insetsPx,
  shrinksAt,
  textRunPx,
  widthClassesAt,
  widthRuleAt,
  wrapsAt,
} from '@/__tests__/utils/tailwind-box';

/**
 * The runs of an element's content that a line never breaks inside, each with its width, read from
 * the rendered DOM by its classes (jsdom lays nothing out; widths and white-space come from
 * `tailwind-box`): the narrowest box the content can stand in is its widest run, so a run wider
 * than its line draws past it, and the page scrolls sideways where the line is the page's edge
 * (readability review 2026-11-18, review-fb-logic finding 2: "#21,045,845 · this device's clock is
 * behind", one 335.4 px run in a 301.5 px phone line).
 *
 * The model breaks only where CSS surely may, so a run is never narrower than the browser's:
 *
 * - at a space whose white-space wraps (`normal`, `pre-wrap`, `pre-line`, `break-spaces`, read
 *   from the nearest element that sets one at the viewport, breakpoint variants included);
 * - at the edges of a block-level box, and between the items of a flex container that wraps.
 *
 * Nothing else breaks: no hyphen, slash or edge of an inline-block. The items of a flex container
 * that does not wrap stand side by side, so its gap joins the last run of one item to the first of
 * the next. An item of a row that cannot shrink (`shrink-0`, `flex-none`) stands at its
 * max-content width, so nothing inside it breaks but a block's edges: its spaces and its own
 * wrapping rows hold (lane B4 fix round: the activity row's right part, a `shrink-0` item, read as
 * its words while it stood as one line). One capped at its container (`max-w-full`) is never wider
 * than its line, so its text breaks as any item's; one that sets its own width otherwise is
 * refused (its box, not its text, stands in the line). So is an item behind `display: contents`
 * (its row is its grandparent's), an item that cannot shrink below its max-content width by its
 * minimum (`min-w-max`, `min-w-fit`) and an item with two width classes (lane B4 second fix
 * round). A replaced element (an `svg`) or an empty sized box (a status dot) is a piece of its
 * run as wide as `tailwind-box` reads it. Spaces collapse to one and a run's end spaces hang.
 *
 * What it cannot read it refuses: a word that may break inside itself where it overflows
 * (`overflow-wrap: anywhere`, `break-words`, `word-break: break-all`), a grid, a grapheme outside the
 * served Latin subset (its advance is a fallback face's, `foreignGraphemes`).
 */

export interface LineRun {
  /** The run's text (a replaced element in it as `▯`). */
  text: string;
  /** Its width in px. */
  px: number;
}

interface Piece {
  ch: string;
  px: number;
  /** A space the line may break at. */
  wraps: boolean;
}

const BLOCK_LEVEL = new Set(['block', 'flex', 'grid', 'list-item', 'table']);
const REPLACED = new Set(['svg', 'img', 'canvas', 'video', 'iframe', 'input', 'select']);

export function lineRuns(root: Element, viewport: number): LineRun[] {
  const runs: LineRun[] = [];
  let pieces: Piece[] = [];
  /** Inside an item of a row that cannot shrink: no space or wrapping row breaks. */
  let rigid = 0;
  /** Whether an element is an item of a flex row that cannot shrink (refused with a set width). */
  const stands = (el: Element) => {
    const parent = el.parentElement;
    if (!parent) return false;
    const display = displayAt(parent, viewport);
    if (display === 'contents')
      throw new Error(
        `line-runs: <${el.tagName.toLowerCase()} class="${el.className}"> stands behind display: contents; the flex row it is an item of is not read`
      );
    if (display !== 'flex' && display !== 'inline-flex') return false;
    const min = minWidthAt(el, viewport);
    if (min === 'max-content' || min === 'fit-content')
      throw new Error(
        `line-runs: <${el.tagName.toLowerCase()} class="${el.className}"> cannot shrink below its max-content width (min-width ${min}); not read`
      );
    if (flexColumnAt(parent, viewport) || shrinksAt(el, viewport)) return false;
    const widths = widthClassesAt(el, viewport);
    if (widths.length > 1)
      throw new Error(
        `line-runs: <${el.tagName.toLowerCase()} class="${el.className}"> has two width classes (${widths.join(', ')}); which holds is not read`
      );
    const width = widthRuleAt(el, viewport);
    // capped at its container (`max-w-full`), it is never wider than its line: its text breaks
    if (width === 'capped') return false;
    if (width === 'set')
      throw new Error(
        `line-runs: <${el.tagName.toLowerCase()} class="${el.className}"> cannot shrink and has a set width; its box, not its text, stands in the line`
      );
    return true;
  };

  const flush = () => {
    // a run's end spaces hang past the line (white-space: normal) or collapse at its edge
    const drawn = (p: Piece) => p.ch !== ' ' && p.ch !== '';
    const first = pieces.findIndex(drawn);
    if (first >= 0) {
      const last = pieces.length - 1 - [...pieces].reverse().findIndex(drawn);
      const kept = pieces.filter((p, i) => p.ch !== ' ' || (i > first && i < last));
      runs.push({
        text: kept.map((p) => p.ch).join(''),
        px: kept.reduce((sum, p) => sum + p.px, 0),
      });
    }
    pieces = [];
  };
  const push = (piece: Piece) => {
    // white-space collapses a space after a space, whether the line wraps there or not
    const last = pieces[pieces.length - 1];
    if (piece.ch === ' ' && (pieces.length === 0 || last.ch === ' ')) return;
    if (piece.ch === ' ' && piece.wraps) {
      flush();
      return;
    }
    pieces.push(piece);
  };

  const text = (node: Text) => {
    const parent = node.parentElement!;
    const value = (node.textContent ?? '').replace(/[\t\n\r ]+/g, ' ');
    if (!value) return;
    if (breaksWordsAt(parent, viewport))
      throw new Error(
        `line-runs: "${value.trim()}" may break inside its words where it overflows; its runs have no width to read`
      );
    const wraps = rigid === 0 && wrapsAt(parent, viewport);
    const glyph = textRunPx('x', parent, viewport);
    for (const { grapheme, units } of foreignGraphemes(value)) {
      if (units !== MONO_ADVANCE_EM * 1000)
        throw new Error(
          `line-runs: "${grapheme}" is outside the served Latin subset; its advance is a fallback face's`
        );
      push({ ch: grapheme === ' ' ? ' ' : grapheme, px: glyph, wraps: grapheme === ' ' && wraps });
    }
  };

  const walk = (node: Node) => {
    if (node.nodeType === Node.TEXT_NODE) return text(node as Text);
    if (!(node instanceof Element)) return;
    if (!inFlowAt(node, viewport)) return;
    const tag = node.tagName.toLowerCase();
    if (REPLACED.has(tag) || (node.childNodes.length === 0 && boxPx(node, viewport)! > 0)) {
      pieces.push({
        ch: '▯',
        px: boxPx(node, viewport)! + insetsPx(node, viewport).margin,
        wraps: false,
      });
      return;
    }
    const display = displayAt(node, viewport);
    if (display === 'grid' || display === 'inline-grid')
      throw new Error(
        `line-runs: <${tag} class="${node.className}"> is a grid; its tracks are not read`
      );
    const block = BLOCK_LEVEL.has(display);
    if (block) flush();
    // an inline box's padding, border and margins stand in its line (a `contents` box has none)
    const { box, margin } =
      display === 'contents' ? { box: 0, margin: 0 } : insetsPx(node, viewport);
    if (box + margin !== 0) pieces.push({ ch: '', px: box + margin, wraps: false });
    if (display === 'flex' || display === 'inline-flex') {
      const wraps = rigid === 0 && flexWrapsAt(node, viewport);
      const gap = gapPx(node, viewport);
      let first = true;
      for (const child of Array.from(node.childNodes)) {
        if (child.nodeType === Node.TEXT_NODE && !(child.textContent ?? '').trim()) continue;
        if (child instanceof Element && !inFlowAt(child, viewport)) continue;
        if (child instanceof Element && displayAt(child, viewport) === 'contents')
          throw new Error(
            `line-runs: <${child.tagName.toLowerCase()} class="${child.className}"> is display: contents in a flex row; its children are the row's items, not read`
          );
        if (!first) {
          if (wraps) flush();
          else if (gap) pieces.push({ ch: '', px: gap, wraps: false });
        }
        first = false;
        const rigidItem = child instanceof Element && stands(child);
        if (rigidItem) rigid++;
        walk(child);
        if (rigidItem) rigid--;
      }
    } else for (const child of Array.from(node.childNodes)) walk(child);
    if (block) flush();
  };

  // the root read alone is an item of its parent's row too
  if (stands(root)) rigid++;
  walk(root);
  flush();
  return runs;
}
