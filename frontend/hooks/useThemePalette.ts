'use client';

import { useTheme } from '@/hooks/useTheme';
import { MATERIALS, PALETTE, type Materials, type Palette } from '@/lib/palette';

export interface ThemePalette {
  palette: Palette;
  materials: Materials;
}

/**
 * The current theme's concrete colours (hex) and materials, for the drawers that cannot read a
 * CSS variable: a 2D canvas, a WebGL shader. Everything else uses `color()` references from
 * lib/palette.ts and lets the stylesheet switch. Re-renders the caller when the theme changes.
 */
export function useThemePalette(): ThemePalette {
  const { resolved } = useTheme();
  return { palette: PALETTE[resolved], materials: MATERIALS[resolved] };
}
