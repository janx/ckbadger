'use client';

import { useId, useRef, type KeyboardEvent, type ReactNode } from 'react';
import { useElementWidth } from '@/hooks/useElementWidth';
import { useFromSm } from '@/hooks/useFromMd';
import { rovingTarget } from '@/lib/roving-focus';
import { groupCount } from '@/lib/format-count';
import { tabRowShowsCounts, type TabRowItem } from '@/lib/control-row-fit';
import Link from '@/components/ui/link';
import { cn } from '@/lib/utils';

/** One tab: its key, the name it writes and its count where it has one. */
export interface Tab<K extends string> extends TabRowItem {
  key: K;
}

interface TabsProps<K extends string> {
  /** The row's name for a screen reader: what its tabs choose between. */
  label: string;
  tabs: readonly Tab<K>[];
  /** The current tab. */
  value: K;
  onChange: (key: K) => void;
  /**
   * The row is a tier-1 section's command line: the prompt square before the first name, the names
   * lowercase. It then stands where the section's `PromptHeading` would (that heading's square is
   * the same `.section-mark`, its command the same step and case), and takes its place: a section
   * opened by a tab row has no heading above it, the current tab is its command.
   */
  command?: boolean;
  /** The current tab's panel. */
  children: ReactNode;
  className?: string;
}

/**
 * A tab's box, the same for a tab and a tab that is its own address (`TabLinks`): the name at the
 * body step below sm and the section step from it, a 32 px target standing on the row's rule, the
 * 2 px line under it (its colour the caller's: celadon for the current one).
 */
const TAB_BOX =
  'text-body sm:text-section -mb-px inline-flex h-8 items-end whitespace-nowrap border-b-2 pb-1.5 transition-colors sm:pb-[7px]';

/** The row under its 1 px rule, one line, a gap of 16 px between tabs (22 from sm). */
const TAB_ROW = 'border-row-rule flex min-w-0 items-baseline gap-4 border-b sm:gap-[22px]';

/** The prompt square before the first name of a command line, pulled toward it. */
const TAB_SQUARE = 'section-mark -mr-2 self-center sm:-mr-3';

/** A count after its tab's name: 12 px in the quiet ink, at every state of its tab. */
function TabCount({ count }: { count: TabRowItem['count'] }) {
  if (count === undefined || count === null) return null;
  return (
    <>
      <span className="sr-only"> </span>
      <span className="text-meta text-text-dim ml-1.5 font-normal">{groupCount(count)}</span>
    </>
  );
}

/**
 * A tab row (the controls board, gate D0′, passed 2026-11-19; UI review 2026-11-14, phase C): one
 * line under a 1 px rule, the names at the section step (17 px) from sm and the body step (14 px)
 * below it, each a 32 px target. A name is the reading ink, the current one bright and 600 on the
 * 2 px celadon line; a count is 12 px in the quiet ink after its name; a zero's name is quiet.
 * WAI-ARIA tabs: `role="tablist"`, one stop in the tab order (the current tab), ← and → move and
 * select (selection follows focus: a panel here is a list the API serves at once, local first),
 * Home and End the first and the last.
 *
 * The phone rule the owner passed: when the row is wider than its line the counts leave it (the
 * list's own count line keeps them); a name is never cut and the row never scrolls or wraps. The
 * row measures its line and lib/control-row-fit.ts decides, the one function, which throws for a row
 * whose names alone do not hold a 360 px window's line.
 */
export function Tabs<K extends string>({
  label,
  tabs,
  value,
  onChange,
  command = false,
  children,
  className,
}: TabsProps<K>) {
  const id = useId();
  const fromSm = useFromSm();
  const [lineRef, linePx] = useElementWidth<HTMLDivElement>();
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const index = tabs.findIndex((tab) => tab.key === value);
  if (index < 0) throw new Error(`Tabs "${label}": no tab ${value}`);
  const counts = tabRowShowsCounts(tabs, fromSm ? 'section' : 'body', command, linePx);
  const tabId = (key: K) => `${id}-tab-${key}`;
  const panelId = `${id}-panel`;

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const to = rovingTarget(event.key, index, tabs.length, 'line');
    if (to === null) return;
    event.preventDefault();
    onChange(tabs[to].key);
    refs.current[to]?.focus();
  };

  return (
    <div className={className}>
      <div
        ref={lineRef}
        role="tablist"
        aria-label={label}
        onKeyDown={onKeyDown}
        className={TAB_ROW}
      >
        {command && <span aria-hidden="true" className={TAB_SQUARE} />}
        {tabs.map((tab, i) => {
          const selected = i === index;
          return (
            <button
              key={tab.key}
              ref={(el) => {
                refs.current[i] = el;
              }}
              type="button"
              role="tab"
              id={tabId(tab.key)}
              aria-selected={selected}
              aria-controls={selected ? panelId : undefined}
              tabIndex={selected ? 0 : -1}
              onClick={() => onChange(tab.key)}
              className={cn(
                TAB_BOX,
                command && 'lowercase',
                selected
                  ? 'border-jade text-text-bright font-semibold'
                  : cn(
                      'hover:text-jade border-transparent font-normal',
                      tab.count === 0 ? 'text-text-dim' : 'text-text'
                    )
              )}
            >
              <span>
                {tab.label}
                {counts && <TabCount count={tab.count} />}
              </span>
            </button>
          );
        })}
      </div>
      <div role="tabpanel" id={panelId} aria-labelledby={tabId(value)} tabIndex={0}>
        {children}
      </div>
    </div>
  );
}

/** One tab that is its own address: its key, its name, its count where it has one, its address. */
export interface TabLink<K extends string> extends Tab<K> {
  href: string;
}

interface TabLinksProps<K extends string> {
  /** The row's name for a screen reader: what its links go between. */
  label: string;
  tabs: readonly TabLink<K>[];
  /** The link of the page this is. */
  current: K;
  /** A tier-1 section's command line, as a tab row's (`TabsProps.command`). */
  command?: boolean;
  className?: string;
}

/**
 * A tab row whose tabs are each their own address (lane c2-finish, item 3): the inventory's
 * classes, three routes. As tabs a class could not be opened in a new tab, and an arrow key changed
 * the route, remounting the page and losing the focus. So the row is a `nav` of links (WAI-ARIA:
 * navigation, not a tablist), the current one `aria-current="page"`, each link a stop of the tab
 * order and the arrows the page's; tabs that choose a view of one page (its query state) stay
 * `Tabs`. The look is the tab row's, box for box (`TAB_ROW`, `TAB_BOX`, the count, the phone rule
 * by `tabRowShowsCounts`).
 *
 * Each link is `look="block"` (components/ui/link.tsx): a block whose content draws its inks, as a
 * tab's name does (the reading ink, the current one bright, a zero's quiet), here on the name's
 * span, whose hover and focus follow the link into the celadon (`group-hover`, `group-focus-visible`,
 * the one rule of app/globals.css). Not `nav`, the shell's own design; not `key` or `onward`, whose
 * one ink each would leave the current tab no brighter than the others.
 */
export function TabLinks<K extends string>({
  label,
  tabs,
  current,
  command = false,
  className,
}: TabLinksProps<K>) {
  const fromSm = useFromSm();
  const [lineRef, linePx] = useElementWidth<HTMLElement>();
  const index = tabs.findIndex((tab) => tab.key === current);
  if (index < 0) throw new Error(`TabLinks "${label}": no link ${current}`);
  const counts = tabRowShowsCounts(tabs, fromSm ? 'section' : 'body', command, linePx);
  return (
    <nav ref={lineRef} aria-label={label} className={cn(TAB_ROW, className)}>
      {command && <span aria-hidden="true" className={TAB_SQUARE} />}
      {tabs.map((tab, i) => {
        const selected = i === index;
        return (
          <Link
            key={tab.key}
            href={tab.href}
            look="block"
            aria-current={selected ? 'page' : undefined}
            className={cn(
              TAB_BOX,
              'group',
              command && 'lowercase',
              selected ? 'border-jade font-semibold' : 'border-transparent font-normal'
            )}
          >
            <span
              className={
                selected
                  ? 'text-text-bright'
                  : cn(
                      'group-hover:text-jade group-focus-visible:text-jade',
                      tab.count === 0 ? 'text-text-dim' : 'text-text'
                    )
              }
            >
              {tab.label}
              {counts && <TabCount count={tab.count} />}
            </span>
          </Link>
        );
      })}
    </nav>
  );
}
