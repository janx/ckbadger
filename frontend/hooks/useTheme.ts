'use client';

import { useSyncExternalStore } from 'react';
import {
  getServerThemeSnapshot,
  getThemeSnapshot,
  setThemePreference,
  subscribeTheme,
  type ThemePreference,
  type ThemeSnapshot,
} from '@/lib/theme';

export interface UseTheme extends ThemeSnapshot {
  setPreference: (preference: ThemePreference) => void;
}

/**
 * The reader's theme preference, the theme the page shows and the setter (lib/theme.ts). Follows
 * a choice made here, in another tab, and the OS scheme while the preference is `system`.
 */
export function useTheme(): UseTheme {
  const snapshot = useSyncExternalStore(subscribeTheme, getThemeSnapshot, getServerThemeSnapshot);
  return { ...snapshot, setPreference: setThemePreference };
}
