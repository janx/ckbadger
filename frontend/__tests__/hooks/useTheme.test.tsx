import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { useTheme } from '@/hooks/useTheme';
import { useThemePalette } from '@/hooks/useThemePalette';
import { MATERIALS, PALETTE } from '@/lib/palette';
import { resetThemeForTests, THEME_STORAGE_KEY } from '@/lib/theme';

describe('useTheme / useThemePalette', () => {
  beforeEach(() => {
    localStorage.clear();
    resetThemeForTests();
  });
  afterEach(() => {
    localStorage.clear();
    resetThemeForTests();
    document.documentElement.setAttribute('data-theme', 'dark');
  });

  it('reads the stored preference on mount and re-renders on a change', () => {
    localStorage.setItem(THEME_STORAGE_KEY, 'light');
    const { result } = renderHook(() => useTheme());
    expect(result.current.preference).toBe('light');
    expect(result.current.resolved).toBe('light');
    expect(document.documentElement.getAttribute('data-theme')).toBe('light');
    act(() => result.current.setPreference('system'));
    // the test OS scheme is dark (__tests__/setup.ts)
    expect(result.current).toMatchObject({
      preference: 'system',
      resolved: 'dark',
      system: 'dark',
    });
    expect(document.documentElement.getAttribute('data-theme')).toBe('dark');
  });

  it('hands a canvas the concrete palette and materials of the theme in force', () => {
    const { result } = renderHook(() => useThemePalette());
    expect(result.current.palette).toBe(PALETTE.dark);
    expect(result.current.materials).toBe(MATERIALS.dark);
    const theme = renderHook(() => useTheme());
    act(() => theme.result.current.setPreference('light'));
    expect(result.current.palette).toBe(PALETTE.light);
    expect(result.current.materials).toBe(MATERIALS.light);
  });
});
