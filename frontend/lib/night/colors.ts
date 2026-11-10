import { hexToRgb, type Palette } from '@/lib/palette';

/** A colour as the shaders take it: three channels in 0..1. */
export type Vec3 = readonly [number, number, number];

/**
 * The night's colours in one theme. The ink and the motes are drawn additively onto `base` at
 * night (`day` = 0); by day (`day` = 1) the shaders subtract each hue's complement from the paper
 * instead, so the same jade reads as jade on white — ink on paper, light on night.
 */
export interface NightColors {
  base: Vec3;
  jade: Vec3;
  aqua: Vec3;
  rose: Vec3;
  violet: Vec3;
  day: 0 | 1;
}

export function vec3(hex: string): Vec3 {
  const { r, g, b } = hexToRgb(hex);
  return [r / 255, g / 255, b / 255];
}

export function nightColors(palette: Palette, day: boolean): NightColors {
  return {
    base: vec3(palette['night-base']),
    jade: vec3(palette.jade),
    aqua: vec3(palette.aqua),
    rose: vec3(palette['night-rose']),
    violet: vec3(palette['night-violet']),
    day: day ? 1 : 0,
  };
}
