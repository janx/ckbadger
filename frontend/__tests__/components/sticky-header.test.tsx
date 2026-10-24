import { afterEach, describe, expect, it, vi } from 'vitest';
import { render } from '../utils/test-utils';
import { Header } from '@/components/layout/header';
import {
  BELOW_STICKY_HEADER,
  STICKY_HEADER_VAR,
  stickyHeaderHeight,
} from '@/components/layout/sticky-header';

vi.mock('@/components/command-palette', () => ({ CommandPalette: () => null }));
vi.mock('@/components/search-bar', () => ({ SearchBar: () => <div>SearchBar</div> }));
vi.mock('@/components/layout/logo', () => ({ Logo: () => <div data-testid="logo" /> }));
vi.mock('@/components/stats-bar', () => ({
  VitalsStrip: () => <div data-testid="vitals-strip" />,
}));

function box(height: number): DOMRect {
  return {
    height,
    width: 0,
    top: 0,
    left: 0,
    right: 0,
    bottom: height,
    x: 0,
    y: 0,
    toJSON: () => ({}),
  };
}

describe('sticky header height', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    document.documentElement.style.removeProperty(STICKY_HEADER_VAR);
  });

  it('is the wrapper (row + strip) where the wrapper sticks, else the header row alone', () => {
    const wrapper = document.createElement('div');
    const header = document.createElement('header');
    wrapper.appendChild(header);
    document.body.appendChild(wrapper);
    vi.spyOn(wrapper, 'getBoundingClientRect').mockReturnValue(box(110));
    vi.spyOn(header, 'getBoundingClientRect').mockReturnValue(box(52));

    // Below md: the wrapper is display: contents and only the row sticks.
    expect(stickyHeaderHeight(wrapper, header)).toBe(52);
    // md and up: the wrapper is the sticky block.
    wrapper.style.position = 'sticky';
    expect(stickyHeaderHeight(wrapper, header)).toBe(110);
    wrapper.remove();
  });

  it('is published by the header as a CSS variable, and cleared when it goes', () => {
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
      this: HTMLElement
    ) {
      return box(this.tagName === 'HEADER' ? 52 : 0);
    });
    const { unmount } = render(<Header />);
    expect(document.documentElement.style.getPropertyValue(STICKY_HEADER_VAR)).toBe('52px');
    unmount();
    expect(document.documentElement.style.getPropertyValue(STICKY_HEADER_VAR)).toBe('');
  });

  it('is what every sticky offset below the header reads', () => {
    expect(BELOW_STICKY_HEADER).toBe(`top-[var(${STICKY_HEADER_VAR})]`);
  });
});
