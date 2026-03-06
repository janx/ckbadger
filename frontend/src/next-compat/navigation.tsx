'use client';

import { useCallback, useContext, useMemo } from 'react';
import {
  UNSAFE_LocationContext,
  UNSAFE_NavigationContext,
  useParams as useRouteParams,
} from 'react-router-dom';

type NavigateOptions = {
  scroll?: boolean;
};

function getWindowPathname(): string {
  return typeof window === 'undefined' ? '/' : window.location.pathname;
}

function getWindowSearch(): string {
  return typeof window === 'undefined' ? '' : window.location.search;
}

export function usePathname(): string {
  const locationContext = useContext(UNSAFE_LocationContext);
  return locationContext?.location.pathname ?? getWindowPathname();
}

export function useSearchParams(): URLSearchParams {
  const locationContext = useContext(UNSAFE_LocationContext);
  const search = locationContext?.location.search ?? getWindowSearch();
  return useMemo(() => new URLSearchParams(search), [search]);
}

export function useParams<T extends Record<string, string | string[]>>() {
  return useRouteParams() as T;
}

export function useRouter() {
  const navigationContext = useContext(UNSAFE_NavigationContext);

  const navigate = useCallback(
    (href: string, replace = false, _options?: NavigateOptions) => {
      if (navigationContext) {
        const navigator = navigationContext.navigator as {
          push: (to: string) => void;
          replace: (to: string) => void;
        };

        if (replace) {
          navigator.replace(href);
          return;
        }

        navigator.push(href);
        return;
      }

      if (typeof window === 'undefined') {
        return;
      }

      if (replace) {
        window.location.replace(href);
        return;
      }

      window.location.assign(href);
    },
    [navigationContext]
  );

  return {
    push: (href: string, options?: NavigateOptions) => navigate(href, false, options),
    replace: (href: string, options?: NavigateOptions) => navigate(href, true, options),
    back: () => typeof window !== 'undefined' && window.history.back(),
    forward: () => typeof window !== 'undefined' && window.history.forward(),
    refresh: () => typeof window !== 'undefined' && window.location.reload(),
    prefetch: async () => {},
  };
}

export function redirect(href: string): never {
  if (typeof window !== 'undefined') {
    window.location.assign(href);
  }
  throw new Error(`redirect:${href}`);
}

export function notFound(): never {
  throw new Error('notFound');
}
