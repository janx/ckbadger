'use client';

import { Fragment, useLayoutEffect, useRef, useState, type RefObject } from 'react';
import { NoBreak } from '@/components/ui/no-break';
import { useCopy } from '@/hooks/useCopy';
import { cn } from '@/lib/utils';
import { MONO_ADVANCE_EM } from '@/lib/type-metrics';

const HEX_ID = /^0x[0-9a-fA-F]+$/;
const ADDRESS = /^ck[bt]1[02-9ac-hj-np-z]+$/;

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
 * hidden page) it stays one line. The text and the title give the whole id once; `copyable`
 * copies it whole through the one copy path, the tick standing over the id while the id keeps
 * its lines, so a head of several lines does not shut for the moment and move the page.
 */
export function HeadId({
  value,
  kind,
  copyable = false,
}: {
  value: string;
  kind: 'hash' | 'address';
  copyable?: boolean;
}) {
  if (kind === 'hash' && !HEX_ID.test(value)) throw new Error(`HeadId: ${value} is not a hex id`);
  if (kind === 'address' && !ADDRESS.test(value))
    throw new Error(`HeadId: ${value} is not an address`);
  const [ref, perLine] = useCharactersPerLine();
  const { copied, copy } = useCopy(value);
  const lines = headLines(value, kind === 'hash', perLine ?? value.length);
  return (
    <span
      ref={ref}
      data-head-id=""
      title={value}
      onClick={copyable ? copy : undefined}
      className={cn(
        'relative block min-w-0 flex-1 font-mono',
        copyable && 'hover-lit cursor-pointer transition-colors'
      )}
    >
      <span className={cn(copied && 'invisible')}>
        {lines.map((line, i) => (
          <Fragment key={i}>
            {i > 0 && <br />}
            <NoBreak data-head-line="" className={cn(i > 0 && kind === 'hash' && 'pl-[2ch]')}>
              {line}
            </NoBreak>
          </Fragment>
        ))}
      </span>
      {copied && <span className="absolute left-0 top-0">✓</span>}
    </span>
  );
}
