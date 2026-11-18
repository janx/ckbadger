'use client';

import { forwardRef, useContext } from 'react';
import { UNSAFE_LocationContext, UNSAFE_NavigationContext } from 'react-router-dom';
import { prefixNetwork, resolveActiveNetwork } from '@/lib/active-network';

export interface LinkProps extends React.AnchorHTMLAttributes<HTMLAnchorElement> {
  href: string;
  replace?: boolean;
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
  { href, replace = false, children, ...props },
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
    <a ref={ref} href={target} {...props} onClick={handleClick}>
      {children}
    </a>
  );
});

export default Link;
