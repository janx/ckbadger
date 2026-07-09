'use client';

import { forwardRef, useContext } from 'react';
import { UNSAFE_LocationContext, UNSAFE_NavigationContext } from 'react-router-dom';
import { prefixNetwork, resolveActiveNetwork } from '@/lib/active-network';

export interface LinkProps extends React.AnchorHTMLAttributes<HTMLAnchorElement> {
  href: string;
  replace?: boolean;
}

function isExternalHref(href: string): boolean {
  return /^(https?:)?\/\//.test(href) || href.startsWith('mailto:') || href.startsWith('tel:');
}

export const Link = forwardRef<HTMLAnchorElement, LinkProps>(function Link(
  { href, replace = false, children, ...props },
  ref
) {
  const navigationContext = useContext(UNSAFE_NavigationContext);
  const locationContext = useContext(UNSAFE_LocationContext);
  // Derive the active network from the router location (falls back to the window
  // path when rendered outside a router). External / already-prefixed hrefs are
  // returned unchanged by prefixNetwork.
  const net = resolveActiveNetwork(
    locationContext?.location.pathname ??
      (typeof window === 'undefined' ? '/' : window.location.pathname)
  );
  const target = prefixNetwork(href, net);

  const handleClick: React.MouseEventHandler<HTMLAnchorElement> = (event) => {
    props.onClick?.(event);

    if (
      event.defaultPrevented ||
      !navigationContext ||
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

    const navigator = navigationContext.navigator as {
      push: (to: string) => void;
      replace: (to: string) => void;
    };

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
