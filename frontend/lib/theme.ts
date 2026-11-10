/**
 * The theme: which of the two palettes (lib/palette.ts) the page is drawn in.
 *
 * The reader's choice is a preference — `dark`, `light` or `system` (follow the OS) — kept in
 * localStorage under `THEME_STORAGE_KEY` and resolved to the theme the page shows, which is the
 * `data-theme` attribute on <html> that every CSS variable keys on. The dark theme (midnight, the
 * one the site was designed in) is the default until the reader chooses. index.html carries a
 * tiny inline copy of the resolution so the first paint is already in the chosen theme; this
 * module is the one that runs afterwards: `useTheme` (hooks/useTheme.ts) subscribes to it.
 */

import type { ThemeName } from '@/lib/palette';

export const THEME_PREFERENCES = ['dark', 'light', 'system'] as const;
export type ThemePreference = (typeof THEME_PREFERENCES)[number];
export type ResolvedTheme = ThemeName;

export const THEME_STORAGE_KEY = 'ckbadger.theme';
export const DEFAULT_THEME_PREFERENCE: ThemePreference = 'dark';
export const LIGHT_SCHEME_QUERY = '(prefers-color-scheme: light)';

export interface ThemeSnapshot {
  preference: ThemePreference;
  resolved: ResolvedTheme;
  /** What `system` resolves to right now. */
  system: ResolvedTheme;
}

export function isThemePreference(value: unknown): value is ThemePreference {
  return (THEME_PREFERENCES as readonly unknown[]).includes(value);
}

/** The preference a stored value names; anything else (nothing stored, an old value) is the default. */
export function parseThemePreference(stored: string | null | undefined): ThemePreference {
  return isThemePreference(stored) ? stored : DEFAULT_THEME_PREFERENCE;
}

export function resolveTheme(preference: ThemePreference, system: ResolvedTheme): ResolvedTheme {
  return preference === 'system' ? system : preference;
}

/** The OS colour scheme, light when it says so and dark otherwise (and where it cannot be asked). */
export function systemTheme(
  win: Pick<Window, 'matchMedia'> | undefined = typeof window === 'undefined' ? undefined : window
): ResolvedTheme {
  if (!win || typeof win.matchMedia !== 'function') return 'dark';
  return win.matchMedia(LIGHT_SCHEME_QUERY).matches ? 'light' : 'dark';
}

function storage(): Pick<Storage, 'getItem' | 'setItem'> | null {
  try {
    return typeof window === 'undefined' ? null : window.localStorage;
  } catch {
    // storage access can throw (a blocked third-party context, a hardened browser)
    return null;
  }
}

/** The stored preference, or the default when nothing valid is stored or storage is unavailable. */
export function readThemePreference(
  store: Pick<Storage, 'getItem'> | null = storage()
): ThemePreference {
  try {
    return parseThemePreference(store?.getItem(THEME_STORAGE_KEY));
  } catch {
    return DEFAULT_THEME_PREFERENCE;
  }
}

/** Sets the theme the page shows: the attribute every colour variable keys on. */
export function applyTheme(
  resolved: ResolvedTheme,
  root: HTMLElement | null = typeof document === 'undefined' ? null : document.documentElement
): void {
  if (root && root.getAttribute('data-theme') !== resolved)
    root.setAttribute('data-theme', resolved);
}

const SERVER_SNAPSHOT: ThemeSnapshot = { preference: 'dark', resolved: 'dark', system: 'dark' };

let snapshot: ThemeSnapshot | null = null;
const listeners = new Set<() => void>();
let unwatch: (() => void) | null = null;

function compute(): ThemeSnapshot {
  const preference = readThemePreference();
  const system = systemTheme();
  return { preference, system, resolved: resolveTheme(preference, system) };
}

/**
 * Recomputes from storage and the OS; applies the result; tells the listeners when it changed
 * (the first reading is a start, not a change).
 */
function refresh(): ThemeSnapshot {
  const previous = snapshot;
  const next = compute();
  const changed =
    !previous ||
    previous.preference !== next.preference ||
    previous.resolved !== next.resolved ||
    previous.system !== next.system;
  if (changed) snapshot = next;
  applyTheme(next.resolved);
  if (previous && changed) for (const listener of listeners) listener();
  return snapshot!;
}

/** The current theme, as a stable object until something changes (for useSyncExternalStore). */
export function getThemeSnapshot(): ThemeSnapshot {
  if (typeof window === 'undefined') return SERVER_SNAPSHOT;
  return snapshot ?? refresh();
}

export function getServerThemeSnapshot(): ThemeSnapshot {
  return SERVER_SNAPSHOT;
}

/** Stores the reader's choice, draws the page in it and tells the listeners. */
export function setThemePreference(preference: ThemePreference): void {
  if (!isThemePreference(preference))
    throw new Error(`theme: ${String(preference)} is not a theme preference`);
  try {
    storage()?.setItem(THEME_STORAGE_KEY, preference);
  } catch {
    // the choice still applies for this page; it is only not remembered
  }
  refresh();
}

function watch(): () => void {
  const onStorage = (event: StorageEvent) => {
    if (event.key === null || event.key === THEME_STORAGE_KEY) refresh();
  };
  window.addEventListener('storage', onStorage);
  const mql =
    typeof window.matchMedia === 'function' ? window.matchMedia(LIGHT_SCHEME_QUERY) : null;
  const onScheme = () => refresh();
  mql?.addEventListener('change', onScheme);
  return () => {
    window.removeEventListener('storage', onStorage);
    mql?.removeEventListener('change', onScheme);
  };
}

/**
 * Subscribes to theme changes: the reader's choice here, the same choice in another tab, the OS
 * scheme while the preference is `system`.
 */
export function subscribeTheme(listener: () => void): () => void {
  listeners.add(listener);
  if (typeof window !== 'undefined') {
    // the baseline the listener will be told about changes from
    if (!snapshot) refresh();
    if (!unwatch) unwatch = watch();
  }
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0 && unwatch) {
      unwatch();
      unwatch = null;
    }
  };
}

/**
 * Test seam: forgets the cached snapshot (the next read recomputes from storage and the OS) and
 * drops every listener and the window watchers, so one test's state never reaches the next.
 */
export function resetThemeForTests(): void {
  snapshot = null;
  listeners.clear();
  if (unwatch) {
    unwatch();
    unwatch = null;
  }
}
