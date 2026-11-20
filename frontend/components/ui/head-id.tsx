'use client';

import {
  forwardRef,
  Fragment,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
  type RefObject,
} from 'react';
import { CopyButton } from '@/components/ui/copy-button';
import { NoBreak } from '@/components/ui/no-break';
import { cn } from '@/lib/utils';
import { MONO_ADVANCE_EM } from '@/lib/type-metrics';

const HEX_ID = /^0x[0-9a-fA-F]+$/;
const ADDRESS = /^ck[bt]1[02-9ac-hj-np-z]+$/;
const DID = /^did:ckb:[a-z2-7]+$/;

/**
 * The lines a full id is set in at `perLine` characters a line: the id alone when it fits;
 * otherwise as few equal lines as hold it, the last the remainder. A hex id keeps `0x` on its
 * first line and indents the later ones 2ch, so its digits stand under its digits: each line takes
 * 2 + k character widths, k = ceil(digits / n) — a 66-character hash in two, 34 + 32. An address
 * has no prefix to stand under: lines of k = ceil(length / n) characters.
 */
function headLines(value: string, hex: boolean, perLine: number): string[] {
  if (value.length <= perLine) return [value];
  const prefix = hex ? 2 : 0;
  const body = value.slice(prefix);
  const room = perLine - prefix;
  // A line narrower than one designed line (`0x` and a digit; an address's one character) is a
  // layout state, not a broken invariant (A4 round 2): it cannot be designed, so the id stands as
  // it does before it is measured, one line, whole.
  if (room < 1) return [value];
  const k = Math.ceil(body.length / Math.ceil(body.length / room));
  const lines: string[] = [];
  for (let at = 0; at < body.length; at += k) lines.push(body.slice(at, at + k));
  lines[0] = value.slice(0, prefix) + lines[0];
  return lines;
}

/**
 * The characters a line of the id holds: the room of its line over a character's advance, 0.6 em
 * of its computed font size plus its tracking, both read from the computed style (the head sets the
 * voice: a name plate's id line 13 px, an asset head's 12 px). The room is the id's own width where
 * it stands alone (a block); beside its copy it is the row's width less the copy's box and the
 * row's gap, since the id takes its content's width there and its own box measures only what it
 * already holds (lane c2-finish, rulings round 2). Null until measured: no width or no type to read
 * (before layout, a hidden page, jsdom) is no measurement. Measured in a layout effect and again on
 * every resize of the line.
 */
function useCharactersPerLine(): [
  RefObject<HTMLSpanElement | null>,
  RefObject<HTMLSpanElement | null>,
  number | null,
] {
  const idRef = useRef<HTMLSpanElement>(null);
  const rowRef = useRef<HTMLSpanElement>(null);
  const [perLine, setPerLine] = useState<number | null>(null);
  useLayoutEffect(() => {
    const id = idRef.current;
    if (!id) return;
    const row = rowRef.current;
    const line = row ?? id;
    const read = () => {
      let room = line.getBoundingClientRect().width;
      if (row) {
        const copy = row.lastElementChild;
        if (!copy || copy === id) throw new Error('HeadId: a row with no copy after its id');
        room -=
          copy.getBoundingClientRect().width + parseFloat(window.getComputedStyle(row).columnGap);
      }
      const style = window.getComputedStyle(id);
      const fontSize = parseFloat(style.fontSize);
      const tracking = style.letterSpacing === 'normal' ? 0 : parseFloat(style.letterSpacing);
      if (!(room > 0) || !(fontSize > 0) || Number.isNaN(tracking)) return;
      // the epsilon keeps an exact fit (66 × 7.2 = 475.2) whole
      setPerLine(Math.floor((room + 1e-6) / (MONO_ADVANCE_EM * fontSize + tracking)));
    };
    read();
    const ro = new ResizeObserver(read);
    ro.observe(line);
    return () => ro.disconnect();
  }, []);
  return [idRef, rowRef, perLine];
}

/**
 * An id line's one form with its copy (lane c2-finish): the id, then the library's CopyButton 8 px
 * after it at its first line. The id takes its content's width (it may shrink and wrap; it never
 * grows), so the copy stands right after it, not at the far end of the row; the copy stands the
 * row's full height, so its answer hangs under the whole id. HeadId draws it, and so does an id
 * line that is not one of HeadId's kinds (the cell's out point).
 */
export const IdCopyRow = forwardRef<
  HTMLSpanElement,
  { value: string; copy: string; children: ReactNode }
>(function IdCopyRow({ value, copy, children }, ref) {
  return (
    <span ref={ref} className="flex min-w-0 items-start gap-2">
      {children}
      <CopyButton value={value} label={copy} className="shrink-0 self-stretch" />
    </span>
  );
});

/**
 * A full id in a page's head: a name plate's id line (block, transaction, uncle, address) or an
 * asset head's id under its title (token, cluster, class, object, m-NFT) — the lead's ruling a,
 * widened to every head (A4 Chrome F3). Never a sideways scroller (RESPONSIVE_LAYOUT principle 1:
 * the head is the first screen) and never broken where the browser pleases. ONE line where the
 * measured line holds it; otherwise equal designed lines, each one unit (`headLines`): a hash as
 * the plate's two halves, `0x` and 32 digits, then 32 under them; a 97-character address in two
 * or three lines at a phone's width; a Godwoken lock's 600-odd characters in as many as it needs.
 * Full ids in the evidence tier and in tables keep HexCode's own-box scroller.
 *
 * Measured (principle 4), not a breakpoint: alone it is a block as wide as its line; beside its
 * copy it measures the row's line less the copy's box and the gap, and takes its content's width;
 * it sets no voice of its own (the head's line does: 13 px is 7.8 px a character, 12 px 7.2). It
 * is the one-line form
 * until measured: the measure is a layout effect, which re-renders before the browser's first
 * paint, so the one-line form is never painted where it does not fit; unmeasured (no layout, a
 * hidden page) it stays one line. The text and the title give the whole id once.
 *
 * Copy has one form (lane c2-finish, item 4; the controls board): the id is a value, text in its
 * line's ink, and `copy` (the button's name, `copy block hash`) puts the library's CopyButton after
 * it, 8 px apart, its icon at the id's first line (`IdCopyRow`: the id takes its content's width,
 * so the copy stands right after it, not at the row's far end: lane c2-finish, rulings round 2);
 * it copies the id whole. Its answer takes no room
 * in the line (lane c2-finish, rulings round 1): the copy stands the row's full height, so the
 * answer hangs under the whole id, over none of its lines. The id measures the line the button
 * leaves it, the same whatever the answer says: its box (24 px from 640, 32 below) and the 8 px
 * gap, 32 px or 40. A hash at 13 px keeps the lines it had alone: one at 640 (576 of 608 px holds
 * 514.8) and from md beside a stone, two halves at 390 (318 holds a half's 265.2). A did:ckb (kind
 * `did`) is set as an address is, in equal lines with no prefix.
 */
export function HeadId({
  value,
  kind,
  copy,
}: {
  value: string;
  kind: 'hash' | 'address' | 'did';
  /** The copy button's name, which puts it after the id (`copy block hash`); none, no copy. */
  copy?: string;
}) {
  if (kind === 'hash' && !HEX_ID.test(value)) throw new Error(`HeadId: ${value} is not a hex id`);
  if (kind === 'address' && !ADDRESS.test(value))
    throw new Error(`HeadId: ${value} is not an address`);
  if (kind === 'did' && !DID.test(value)) throw new Error(`HeadId: ${value} is not a did:ckb`);
  const [idRef, rowRef, perLine] = useCharactersPerLine();
  const lines = headLines(value, kind === 'hash', perLine ?? value.length);
  const id = (
    <span ref={idRef} data-head-id="" title={value} className="block min-w-0 font-mono">
      {lines.map((line, i) => (
        <Fragment key={i}>
          {i > 0 && <br />}
          <NoBreak data-head-line="" className={cn(i > 0 && kind === 'hash' && 'pl-[2ch]')}>
            {line}
          </NoBreak>
        </Fragment>
      ))}
    </span>
  );
  if (copy === undefined) return id;
  return (
    <IdCopyRow ref={rowRef} value={value} copy={copy}>
      {id}
    </IdCopyRow>
  );
}
