import type { Config } from 'tailwindcss';
import plugin from 'tailwindcss/plugin';
import { TYPE_SCALE } from './lib/type-scale';
import { glowAlpha, themeBaseStyles, twColor } from './lib/palette';

/**
 * A glow shadow: two halos in one hue, their alphas scaled by the theme's glow factor
 * (`--m-glow`: 1 at night, 0 by day — see lib/palette.ts).
 */
const glow = (key: Parameters<typeof glowAlpha>[0], near: number, far: number) =>
  `0 0 10px ${glowAlpha(key, near)}, 0 0 20px ${glowAlpha(key, far)}`;

export default {
  content: ['./app/**/*.{js,ts,jsx,tsx,mdx}', './components/**/*.{js,ts,jsx,tsx,mdx}'],
  // The theme is the `data-theme` attribute on <html> (lib/theme.ts); `dark:` variants, should a
  // page ever need one, follow it rather than the OS.
  darkMode: ['selector', '[data-theme="dark"]'],
  theme: {
    // The one type scale (hierarchy plan R1, S8): `text-meta` … `text-display`, defined in lib. Set
    // as the whole theme, not under `extend`, so Tailwind's default sizes (text-xs …) do not exist.
    fontSize: TYPE_SCALE,
    container: {
      center: true,
      // The page gutter (material pass v2, move 6): 16 px, 24 px from lg. Every `container`
      // takes it from here; a page adds no `px-*` of its own.
      padding: {
        DEFAULT: '1rem',
        lg: '1.5rem',
      },
      screens: {
        sm: '640px',
        md: '768px',
        lg: '1024px',
        xl: '1280px',
      },
    },
    extend: {
      // Every colour resolves through the theme's CSS variables (lib/palette.ts declares both
      // themes; `twColor` keeps the class's opacity modifier working). The key names are the
      // roles the pages were written against; the values are the rubbing by night and the Song
      // print by day (jade = celadon, aqua = indigo, rouge = cinnabar text, lavender = lotus,
      // amber = moss; `mark` is cinnabar itself — see frontend/docs/ui-color-guidelines.md).
      colors: {
        base: {
          void: twColor('base-void'),
          bg: twColor('base-bg'),
          surface: twColor('base-surface'),
          elevated: twColor('base-elevated'),
          border: twColor('base-border'),
          'border-subtle': twColor('base-border-subtle'),
        },
        text: {
          bright: twColor('text-bright'),
          DEFAULT: twColor('text'),
          // ≥ 4.5:1 on base.surface and base.bg in both themes (__tests__/lib/palette.test.ts).
          dim: twColor('text-dim'),
          ghost: twColor('text-ghost'),
        },
        interactive: {
          DEFAULT: twColor('aqua'),
          hover: twColor('jade'),
          muted: 'rgb(var(--rgb-aqua) / 0.125)',
          dim: twColor('aqua-dim'),
        },
        emphasis: {
          DEFAULT: twColor('jade'),
          dim: twColor('jade-dim'),
          glow: 'rgb(var(--rgb-jade) / 0.25)',
          bright: twColor('jade-bright'),
        },
        positive: {
          DEFAULT: twColor('jade'),
          dim: twColor('jade-dim'),
          bright: twColor('jade-bright'),
        },
        negative: {
          DEFAULT: twColor('rouge'),
          dim: twColor('rouge-dim'),
          bright: twColor('rouge-bright'),
        },
        warning: {
          DEFAULT: twColor('gold'),
          dim: twColor('gold-dim'),
          bright: twColor('gold-bright'),
        },
        info: {
          DEFAULT: twColor('aqua'),
          dim: twColor('aqua-dim'),
          bright: twColor('aqua-bright'),
        },
        // Chinese traditional named colors
        jade: {
          DEFAULT: twColor('jade'),
          dim: twColor('jade-dim'),
        },
        rouge: {
          DEFAULT: twColor('rouge'),
          dim: twColor('rouge-dim'),
        },
        aqua: {
          DEFAULT: twColor('aqua'),
          dim: twColor('aqua-dim'),
        },
        gold: {
          DEFAULT: twColor('gold'),
          dim: twColor('gold-dim'),
        },
        lavender: {
          DEFAULT: twColor('lavender'),
          dim: twColor('lavender-dim'),
        },
        amber: {
          DEFAULT: twColor('amber'),
          dim: twColor('amber-dim'),
        },
        // Cinnabar, the mark (UI grade, ≥ 3:1): prompt squares, seals, the search square — never
        // body text (text in cinnabar is `rouge`).
        mark: twColor('mark'),
        // Activity type semantic colors
        token: {
          DEFAULT: twColor('token'),
          bright: twColor('token-bright'),
        },
        object: twColor('object'),
        identity: twColor('identity'),
        copper: twColor('copper'),
        // Legacy chart aliases
        'accent-1': twColor('jade'),
        'accent-2': twColor('rouge'),
        'accent-3': twColor('aqua'),
        'accent-4': twColor('gold'),
        'accent-5': twColor('lavender'),
        'accent-6': twColor('amber'),
        'accent-7': twColor('jade-dim'),
        'accent-8': twColor('rouge-dim'),
        'accent-9': twColor('aqua-dim'),
        'accent-10': twColor('gold-dim'),
        'accent-11': twColor('lavender-dim'),
        'accent-12': twColor('amber-dim'),
      },
      fontFamily: {
        mono: ['var(--font-mono)', 'ui-monospace', 'monospace'],
        display: ['var(--font-mono)', 'ui-monospace', 'monospace'],
      },
      fontFeatureSettings: {
        tnum: '"tnum"',
      },
      boxShadow: {
        glow: `0 0 8px ${glowAlpha('jade', 0.125)}, 0 0 20px ${glowAlpha('jade', 0.07)}`,
        'glow-strong': `0 0 8px ${glowAlpha('jade', 0.19)}, 0 0 20px ${glowAlpha('jade', 0.125)}`,
        'glow-inset': `inset 0 1px 10px ${glowAlpha('jade', 0.063)}`,
        'glow-jade': glow('jade', 0.157, 0.078),
        'glow-rouge': glow('rouge', 0.157, 0.078),
        'glow-aqua': glow('aqua', 0.157, 0.078),
        'glow-gold': glow('gold', 0.157, 0.078),
        'glow-lavender': glow('lavender', 0.157, 0.078),
        'glow-amber': glow('amber', 0.157, 0.078),
      },
      animation: {
        'terminal-flicker': 'terminal-flicker 0.15s infinite',
        'terminal-glow-pulse': 'terminal-glow-pulse 4s ease-in-out infinite',
        'digit-tick': 'digit-tick 0.2s ease-out',
        'scan-line': 'scan-line 8s linear infinite',
        'text-reveal': 'text-reveal 0.5s ease-out forwards',
        // Micro-interactions
        'glow-pulse': 'glow-pulse 1.5s ease-in-out infinite',
        'confirm-fade': 'confirm-fade 0.3s cubic-bezier(0.25, 1, 0.5, 1)',
        glitch: 'glitch 0.3s ease-out',
      },
      keyframes: {
        'terminal-flicker': {
          '0%, 100%': { opacity: '1' },
          '50%': { opacity: '0.99' },
          '25%, 75%': { opacity: '0.98' },
        },
        'terminal-glow-pulse': {
          '0%, 100%': { filter: 'brightness(1)' },
          '50%': { filter: 'brightness(1.05)' },
        },
        'digit-tick': {
          '0%': { transform: 'translateY(-100%)', opacity: '0' },
          '100%': { transform: 'translateY(0)', opacity: '1' },
        },
        'scan-line': {
          '0%': { transform: 'translateY(-100%)' },
          '100%': { transform: 'translateY(100vh)' },
        },
        'text-reveal': {
          '0%': { opacity: '0', filter: 'blur(4px)' },
          '100%': { opacity: '1', filter: 'blur(0)' },
        },
        // Micro-interaction keyframes
        'glow-pulse': {
          '0%, 100%': { opacity: '0.5' },
          '50%': { opacity: '1' },
        },
        'confirm-fade': {
          '0%': { opacity: '0', transform: 'translateY(2px)' },
          '100%': { opacity: '1', transform: 'translateY(0)' },
        },
        glitch: {
          '0%': { transform: 'translate(0)', opacity: '1' },
          '20%': { transform: 'translate(-1px, 1px)', opacity: '0.9' },
          '40%': { transform: 'translate(1px, -1px)', opacity: '0.95' },
          '60%': { transform: 'translate(-0.5px, 0.5px)', opacity: '0.9' },
          '80%': { transform: 'translate(0.5px, -0.5px)', opacity: '0.95' },
          '100%': { transform: 'translate(0)', opacity: '1' },
        },
      },
      // Japanese poster inspired spacing rhythm
      spacing: {
        '18': '4.5rem',
        '22': '5.5rem',
      },
    },
  },
  plugins: [
    // Both themes' variables, as base styles: the dark theme on :root, the light one under
    // :root[data-theme='light'] (lib/palette.ts is the one place the values live).
    plugin(({ addBase }) => {
      addBase(themeBaseStyles());
    }),
  ],
} satisfies Config;
