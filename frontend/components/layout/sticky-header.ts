'use client';

import { useLayoutEffect, type RefObject } from 'react';

/**
 * The site header's sticky height, measured by the header and published on <html> as one CSS
 * variable, so anything that sticks below it (the /activities toolbar) never hard-codes an offset.
 */
export const STICKY_HEADER_VAR = '--sticky-header-h';

/** `top` for an element that sticks right below the site header. */
export const BELOW_STICKY_HEADER = 'top-[var(--sticky-header-h)]';

/**
 * What sticks: from md up the wrapper around the header row and the vitals strip is the sticky
 * block; below md it is display: contents and only the header row sticks.
 */
export function stickyHeaderHeight(wrapper: HTMLElement, header: HTMLElement): number {
  const sticky = getComputedStyle(wrapper).position === 'sticky' ? wrapper : header;
  return sticky.getBoundingClientRect().height;
}

/** Keep STICKY_HEADER_VAR equal to the sticky part of the header while it is mounted. */
export function useStickyHeaderHeight(
  wrapperRef: RefObject<HTMLElement | null>,
  headerRef: RefObject<HTMLElement | null>,
  stripRef: RefObject<HTMLElement | null>
): void {
  useLayoutEffect(() => {
    const wrapper = wrapperRef.current;
    const header = headerRef.current;
    const strip = stripRef.current;
    if (!wrapper || !header || !strip) return;
    const root = document.documentElement;
    const write = () =>
      root.style.setProperty(STICKY_HEADER_VAR, `${stickyHeaderHeight(wrapper, header)}px`);
    write();
    // The row and the strip change height as they wrap; a breakpoint changes what sticks.
    const ro = new ResizeObserver(write);
    ro.observe(header);
    ro.observe(strip);
    window.addEventListener('resize', write);
    return () => {
      ro.disconnect();
      window.removeEventListener('resize', write);
      root.style.removeProperty(STICKY_HEADER_VAR);
    };
  }, [wrapperRef, headerRef, stripRef]);
}
