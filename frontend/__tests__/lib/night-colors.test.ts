import { describe, expect, it } from 'vitest';
import { nightColors, vec3 } from '@/lib/night/colors';
import { PALETTE } from '@/lib/palette';
import { POINT_FS, SHOW } from '@/lib/night/gl';

describe('the night’s colours', () => {
  it('turns a hex into shader channels in 0..1', () => {
    expect(vec3('#ff0000')).toEqual([1, 0, 0]);
    expect(vec3('#000000')).toEqual([0, 0, 0]);
    const [r, g, b] = vec3('#2edba3');
    expect(r).toBeCloseTo(46 / 255, 6);
    expect(g).toBeCloseTo(219 / 255, 6);
    expect(b).toBeCloseTo(163 / 255, 6);
    expect(() => vec3('var(--c-jade)')).toThrow('not #rrggbb');
  });

  it('is light on the night by night and ink on paper by day, in each theme’s own hues', () => {
    const night = nightColors(PALETTE.dark, false);
    expect(night.day).toBe(0);
    expect(night.base).toEqual(vec3(PALETTE.dark['night-base']));
    expect(night.jade).toEqual(vec3(PALETTE.dark.jade));
    expect(night.rose).toEqual(vec3(PALETTE.dark['night-rose']));
    const day = nightColors(PALETTE.light, true);
    expect(day.day).toBe(1);
    expect(day.base).toEqual(vec3(PALETTE.light['night-base']));
    expect(day.violet).toEqual(vec3(PALETTE.light['night-violet']));
  });

  it('is what the shaders take: the base and the day flag, the four hues as uniforms', () => {
    for (const name of ['u_base', 'u_day'])
      expect(SHOW).toContain(`uniform ${'u_base' === name ? 'vec3' : 'float'} ${name}`);
    for (const name of ['u_jade', 'u_aqua', 'u_rose', 'u_violet'])
      expect(POINT_FS).toContain(`uniform vec3 ${name}`);
    expect(POINT_FS).toContain('uniform float u_day');
    // by day the point draws the complement, and the show subtracts it from the paper
    expect(POINT_FS).toContain('mix(c,1.-c,u_day)');
    expect(SHOW).toContain('mix(ink,-ink,u_day)');
  });
});
