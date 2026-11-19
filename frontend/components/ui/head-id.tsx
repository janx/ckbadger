'use client';

import { Fragment, useLayoutEffect, useRef, useState, type RefObject } from 'react';
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
 * The characters a line of the element holds: its width over a character's advance, 0.6 em of
 * its computed font size plus its tracking, both read from the computed style (the head sets the
 * voice: a name plate's id line 12 px, an asset head's 11 px). Null until measured: no width or no
 * type to read (before layout, a hidden page, jsdom) is no measurement. Measured in a layout effect
 * and again on every resize.
 */
function useCharactersPerLine(): [RefObject<HTMLSpanElement | null>, number | null] {
  const ref = useRef<HTMLSpanElement>(null);
  const [perLine, setPerLine] = useState<number | null>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const read = () => {
      const line = el.getBoundingClientRect().width;
      const style = window.getComputedStyle(el);
      const fontSize = parseFloat(style.fontSize);
      const tracking = style.letterSpacing === 'normal' ? 0 : parseFloat(style.letterSpacing);
      if (!(line > 0) || !(fontSize > 0) || Number.isNaN(tracking)) return;
      // the epsilon keeps an exact fit (66 × 7.2 = 475.2) whole
      setPerLine(Math.floor((line + 1e-6) / (MONO_ADVANCE_EM * fontSize + tracking)));
    };
    read();
    const ro = new ResizeObserver(read);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  return [ref, perLine];
}

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
 * Measured (principle 4), not a breakpoint: the span takes the free width of its line (`flex-1`
 * in a row, a block alone), so its width is the line's, not the text's; it sets no voice of its
 * own (the head's line does: 12 px is 7.2 px a character, 11 px 6.6). It is the one-line form
 * until measured: the measure is a layout effect, which re-renders before the browser's first
 * paint, so the one-line form is never painted where it does not fit; unmeasured (no layout, a
 * hidden page) it stays one line. The text and the title give the whole id once.
 *
 * Copy has one form (lane c2-finish, item 4; the controls board): the id is a value, text in its
 * line's ink, and `copy` (the button's name, `copy block hash`) puts the library's CopyButton after
 * it, 8 px apart, its icon at the id's first line; it copies the id whole. Its answer takes no room
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
  const [ref, perLine] = useCharactersPerLine();
  const lines = headLines(value, kind === 'hash', perLine ?? value.length);
  const id = (
    <span ref={ref} data-head-id="" title={value} className="block min-w-0 flex-1 font-mono">
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
    <span className="flex min-w-0 items-start gap-2">
      {id}
      <CopyButton value={value} label={copy} className="shrink-0 self-stretch" />
    </span>
  );
}
