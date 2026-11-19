'use client';

import { forwardRef, useContext } from 'react';
import { UNSAFE_LocationContext, UNSAFE_NavigationContext } from 'react-router-dom';
import { prefixNetwork, resolveActiveNetwork } from '@/lib/active-network';
import type { PaletteKey } from '@/lib/palette';
import { cn } from '@/lib/utils';

/**
 * What a link looks like, by its job (the controls board, gate D0′, item "links"; lane c2-links):
 * the one owner of a link's look. A caller names the job, never the classes; `Link` and
 * `ExternalLink` draw it, and the shared renderers that draw a link (`BlockLink`, `Address`) pass
 * their caller's look through. `__tests__/lib/link-look-gate.test.ts` holds every link to stating
 * one and refuses a link ink or an underline written by hand.
 *
 * - `key`: a row's key, the thing the row is about (a hash, a block number, a name): the bright ink.
 * - `inline`: a link inside a line: it keeps the line's ink and is underlined, 1 px and 3 px under
 *   the text, in the muted ink (`INLINE_UNDERLINE`; the coordinator's ruling of 2026-11-19, which
 *   the owner sees at the next gate): plainly there on every ground, quieter than the words it
 *   stands under, at least 3 : 1 on the four grounds in both themes (pinned in palette.test). The
 *   board's `slate-other` line was 1.37–1.66 : 1 by night and 1.50–1.84 : 1 by day, under a
 *   shape's floor. Under hover and focus the line follows its words into the celadon.
 * - `onward`: a way onward: celadon, never broken, its arrow written here and held to its last word
 *   (` →`; ` ↓` to a place further down this page; `ExternalLink` writes ` ↗` to another site).
 * - `block`: a link that is a block — a tile, a card, a row that is one link, a seal, an SVG mark:
 *   no link ink of its own; what it holds draws its inks.
 * - `nav`: the shell's navigation (the header, its menu, the footer, the wordmark, the skip link):
 *   its own design, written by the shell (components/layout/), and nowhere else.
 *
 * A look sets no size: a link reads at its line's step. Hover and focus are one rule for every look,
 * by night and by day (app/globals.css, `a:focus-visible, a:hover`: celadon); an SVG link keeps its
 * own. Keeping the set this small keeps a page's content at three looks of a link.
 */
export const LINK_LOOKS = ['key', 'inline', 'onward', 'block', 'nav'] as const;

export type LinkLook = (typeof LINK_LOOKS)[number];

/** The ink of a link-inside-a-line's underline: a shape, held at 3 : 1 on every ground. */
export const INLINE_UNDERLINE: PaletteKey = 'ink-muted';

const LOOK_CLASS: Record<LinkLook, string> = {
  key: 'text-text-bright',
  // the line's ink; the underline in INLINE_UNDERLINE (its key written out: a class is a literal)
  inline: 'underline decoration-1 underline-offset-[3px] decoration-[color:var(--c-ink-muted)]',
  onward: 'text-jade whitespace-nowrap',
  block: '',
  nav: '',
};

// The class writes the underline's key out (Tailwind reads a class whole); the key palette.test
// pins at 3 : 1 is the one it draws, or nothing is drawn.
if (!LOOK_CLASS.inline.includes(`decoration-[color:var(--c-${INLINE_UNDERLINE})]`))
  throw new Error(`link: the inline look's underline is not drawn in ${INLINE_UNDERLINE}`);

/** The classes of a look, for the link components alone (Link, ExternalLink). */
export function linkLookClass(look: LinkLook | undefined, className?: string): string | undefined {
  const out = cn(look === undefined ? undefined : LOOK_CLASS[look], className);
  return out === '' ? undefined : out;
}

/** The arrow a way onward writes, held to its last word: down to a place on this page, else on. */
function onwardArrow(href: string) {
  return (
    <span aria-hidden="true" className="whitespace-nowrap">
      {href.startsWith('#') ? ' ↓' : ' →'}
    </span>
  );
}

export interface LinkProps extends React.AnchorHTMLAttributes<HTMLAnchorElement> {
  href: string;
  replace?: boolean;
  /**
   * The link's job (`LINK_LOOKS`). Optional only while the link-look gate's TOLERATED list holds
   * call sites that state none; such a link draws its caller's classes alone, as it did before.
   */
  look?: LinkLook;
}

/** The router's history, as the site's links go through it. */
interface LinkNavigator {
  push: (to: string) => void;
  replace: (to: string) => void;
}

function isExternalHref(href: string): boolean {
  return /^(https?:)?\/\//.test(href) || href.startsWith('mailto:') || href.startsWith('tel:');
}

/**
 * Where an href goes on this site, and the router that takes it there: `url` writes the URL the
 * site's links use for an href (an internal one prefixed with the network of the router's
 * location, or of the window's outside a router; an external or already prefixed one unchanged),
 * and `navigator` is the router's history (null outside a router). Link follows its href through
 * it, and so does the pipeline canvas, which opens a page on a click without an anchor, so one
 * href reaches one URL whichever way it is followed.
 */
export function useSiteLink(): {
  url: (href: string) => string;
  navigator: LinkNavigator | null;
} {
  const navigationContext = useContext(UNSAFE_NavigationContext);
  const locationContext = useContext(UNSAFE_LocationContext);
  // Derive the active network from the router location (falls back to the window
  // path when rendered outside a router).
  const net = resolveActiveNetwork(
    locationContext?.location.pathname ??
      (typeof window === 'undefined' ? '/' : window.location.pathname)
  );
  return {
    url: (href) => prefixNetwork(href, net),
    navigator: navigationContext ? (navigationContext.navigator as LinkNavigator) : null,
  };
}

export const Link = forwardRef<HTMLAnchorElement, LinkProps>(function Link(
  { href, replace = false, look, className, children, ...props },
  ref
) {
  const { url, navigator } = useSiteLink();
  const target = url(href);

  const handleClick: React.MouseEventHandler<HTMLAnchorElement> = (event) => {
    props.onClick?.(event);

    if (
      event.defaultPrevented ||
      !navigator ||
      isExternalHref(href) ||
      // a place on this page: the browser goes to it (the router would only change the URL)
      href.startsWith('#') ||
      props.target === '_blank' ||
      event.button !== 0 ||
      event.metaKey ||
      event.altKey ||
      event.ctrlKey ||
      event.shiftKey
    ) {
      return;
    }

    event.preventDefault();

    if (replace) {
      navigator.replace(target);
      return;
    }

    navigator.push(target);
  };

  return (
    <a
      ref={ref}
      href={target}
      {...props}
      className={linkLookClass(look, className)}
      onClick={handleClick}
    >
      {children}
      {look === 'onward' && onwardArrow(href)}
    </a>
  );
});

export default Link;
