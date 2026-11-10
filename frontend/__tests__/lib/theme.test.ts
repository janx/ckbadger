import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  applyTheme,
  DEFAULT_THEME_PREFERENCE,
  getServerThemeSnapshot,
  getThemeSnapshot,
  isThemePreference,
  LIGHT_SCHEME_QUERY,
  parseThemePreference,
  readThemePreference,
  resetThemeForTests,
  resolveTheme,
  setThemePreference,
  subscribeTheme,
  systemTheme,
  THEME_PREFERENCES,
  THEME_STORAGE_KEY,
} from '@/lib/theme';

/** A media query list whose answer and listeners a test controls. */
function fakeMatchMedia(light: boolean) {
  const listeners = new Set<() => void>();
  const mql = {
    get matches() {
      return light;
    },
    media: LIGHT_SCHEME_QUERY,
    addEventListener: (_: string, fn: () => void) => listeners.add(fn),
    removeEventListener: (_: string, fn: () => void) => listeners.delete(fn),
  };
  return {
    matchMedia: vi.fn((query: string) => {
      if (query !== LIGHT_SCHEME_QUERY) throw new Error(`unexpected query ${query}`);
      return mql as unknown as MediaQueryList;
    }),
    setLight(next: boolean) {
      light = next;
      for (const fn of listeners) fn();
    },
    listeners,
  };
}

describe('the theme preference and its resolution', () => {
  it('names three preferences and parses anything else as the default (dark)', () => {
    expect(THEME_PREFERENCES).toEqual(['dark', 'light', 'system']);
    expect(DEFAULT_THEME_PREFERENCE).toBe('dark');
    for (const p of THEME_PREFERENCES) expect(isThemePreference(p)).toBe(true);
    expect(isThemePreference('blue')).toBe(false);
    expect(isThemePreference(undefined)).toBe(false);
    expect(parseThemePreference('light')).toBe('light');
    expect(parseThemePreference('system')).toBe('system');
    expect(parseThemePreference(null)).toBe('dark');
    expect(parseThemePreference('midnight')).toBe('dark');
  });

  it('resolves system to the OS scheme and a choice to itself', () => {
    expect(resolveTheme('dark', 'light')).toBe('dark');
    expect(resolveTheme('light', 'dark')).toBe('light');
    expect(resolveTheme('system', 'light')).toBe('light');
    expect(resolveTheme('system', 'dark')).toBe('dark');
  });

  it('reads the OS scheme as light only when it says so, dark where it cannot be asked', () => {
    expect(systemTheme(fakeMatchMedia(true) as unknown as Window)).toBe('light');
    expect(systemTheme(fakeMatchMedia(false) as unknown as Window)).toBe('dark');
    expect(systemTheme(undefined)).toBe('dark');
    expect(systemTheme({} as Window)).toBe('dark');
  });

  it('reads the stored preference, and the default when storage is missing, invalid or throws', () => {
    expect(readThemePreference({ getItem: () => 'light' })).toBe('light');
    expect(readThemePreference({ getItem: () => 'nope' })).toBe('dark');
    expect(readThemePreference(null)).toBe('dark');
    expect(
      readThemePreference({
        getItem: () => {
          throw new Error('blocked');
        },
      })
    ).toBe('dark');
  });

  it('applies a theme as the data-theme attribute, leaving an element already there alone', () => {
    const root = document.createElement('div');
    applyTheme('light', root);
    expect(root.getAttribute('data-theme')).toBe('light');
    const spy = vi.spyOn(root, 'setAttribute');
    applyTheme('light', root);
    expect(spy).not.toHaveBeenCalled();
    applyTheme('dark', root);
    expect(root.getAttribute('data-theme')).toBe('dark');
    applyTheme('dark', null);
  });
});

describe('the theme store', () => {
  const originalMatchMedia = window.matchMedia;
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.removeAttribute('data-theme');
    resetThemeForTests();
  });
  afterEach(() => {
    window.matchMedia = originalMatchMedia;
    localStorage.clear();
    resetThemeForTests();
    document.documentElement.setAttribute('data-theme', 'dark');
  });

  it('starts dark, applies the theme to <html> and keeps one snapshot until something changes', () => {
    const first = getThemeSnapshot();
    expect(first).toEqual({ preference: 'dark', resolved: 'dark', system: 'dark' });
    expect(document.documentElement.getAttribute('data-theme')).toBe('dark');
    expect(getThemeSnapshot()).toBe(first);
    expect(getServerThemeSnapshot()).toEqual({
      preference: 'dark',
      resolved: 'dark',
      system: 'dark',
    });
  });

  it('stores a choice, draws the page in it and tells the listeners once', () => {
    const listener = vi.fn();
    const unsubscribe = subscribeTheme(listener);
    setThemePreference('light');
    expect(localStorage.getItem(THEME_STORAGE_KEY)).toBe('light');
    expect(document.documentElement.getAttribute('data-theme')).toBe('light');
    expect(getThemeSnapshot()).toEqual({ preference: 'light', resolved: 'light', system: 'dark' });
    expect(listener).toHaveBeenCalledTimes(1);
    // the same choice again changes nothing and says nothing
    setThemePreference('light');
    expect(listener).toHaveBeenCalledTimes(1);
    unsubscribe();
    setThemePreference('dark');
    expect(listener).toHaveBeenCalledTimes(1);
    expect(document.documentElement.getAttribute('data-theme')).toBe('dark');
  });

  it('refuses a value that is not a preference', () => {
    expect(() => setThemePreference('blue' as never)).toThrow('blue is not a theme preference');
  });

  it('follows the same choice made in another tab', () => {
    const listener = vi.fn();
    const unsubscribe = subscribeTheme(listener);
    expect(getThemeSnapshot().resolved).toBe('dark');
    localStorage.setItem(THEME_STORAGE_KEY, 'light');
    window.dispatchEvent(
      new StorageEvent('storage', { key: THEME_STORAGE_KEY, newValue: 'light' })
    );
    expect(listener).toHaveBeenCalledTimes(1);
    expect(getThemeSnapshot().resolved).toBe('light');
    expect(document.documentElement.getAttribute('data-theme')).toBe('light');
    // another key is not ours
    window.dispatchEvent(new StorageEvent('storage', { key: 'other', newValue: 'x' }));
    expect(listener).toHaveBeenCalledTimes(1);
    unsubscribe();
  });

  it('follows the OS scheme while the preference is system, and not otherwise', () => {
    const media = fakeMatchMedia(false);
    window.matchMedia = media.matchMedia as unknown as typeof window.matchMedia;
    const listener = vi.fn();
    const unsubscribe = subscribeTheme(listener);
    setThemePreference('system');
    expect(getThemeSnapshot()).toEqual({ preference: 'system', resolved: 'dark', system: 'dark' });
    media.setLight(true);
    expect(getThemeSnapshot()).toEqual({
      preference: 'system',
      resolved: 'light',
      system: 'light',
    });
    expect(document.documentElement.getAttribute('data-theme')).toBe('light');
    // a fixed choice reads the OS but does not follow it
    setThemePreference('dark');
    const calls = listener.mock.calls.length;
    media.setLight(false);
    expect(getThemeSnapshot()).toEqual({ preference: 'dark', resolved: 'dark', system: 'dark' });
    expect(listener.mock.calls.length).toBe(calls + 1); // the system reading changed
    expect(document.documentElement.getAttribute('data-theme')).toBe('dark');
    unsubscribe();
    // the last subscriber gone, the OS is no longer watched
    expect(media.listeners.size).toBe(0);
  });
});
