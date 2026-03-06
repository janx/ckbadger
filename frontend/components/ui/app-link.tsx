'use client';

import { forwardRef, useContext } from 'react';
import { UNSAFE_NavigationContext } from 'react-router-dom';

interface AppLinkProps extends React.AnchorHTMLAttributes<HTMLAnchorElement> {
  href: string;
  replace?: boolean;
}

function isExternalHref(href: string): boolean {
  return /^(https?:)?\/\//.test(href) || href.startsWith('mailto:') || href.startsWith('tel:');
}

export const AppLink = forwardRef<HTMLAnchorElement, AppLinkProps>(function AppLink(
  { href, replace = false, children, ...props },
  ref
) {
  const navigationContext = useContext(UNSAFE_NavigationContext);

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
      navigator.replace(href);
      return;
    }

    navigator.push(href);
  };

  return (
    <a ref={ref} href={href} {...props} onClick={handleClick}>
      {children}
    </a>
  );
});
