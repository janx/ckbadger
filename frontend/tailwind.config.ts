import type { Config } from 'tailwindcss';

export default {
  content: ['./app/**/*.{js,ts,jsx,tsx,mdx}', './components/**/*.{js,ts,jsx,tsx,mdx}'],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        ckb: {
          primary: '#00c389',
          secondary: '#3cc68a',
          dark: '#0a0f1a',
          darker: '#060b14',
        },
        // Fallout-style phosphor green palette
        terminal: {
          green: '#00ff41', // Bright phosphor green (primary glow)
          dim: '#00cc33', // Dimmed green for secondary text
          dark: '#00801f', // Dark green for borders/accents
          glow: '#00ff4180', // Green with 50% opacity for glow effects
          bg: '#0a100a', // Near-black with green tint
          'bg-light': '#0d140d', // Slightly lighter terminal background
        },
        // Fallout amber accent palette
        amber: {
          DEFAULT: '#ffb000', // Primary amber (Pip-Boy style)
          bright: '#ffc832', // Highlight amber
          dim: '#cc8c00', // Muted amber
          glow: '#ffb00080', // Amber with 50% opacity
          dark: '#805800', // Dark amber for borders
        },
        // Japanese poster inspired slate/blue-gray
        slate: {
          950: '#0a0d12', // Deepest background
          900: '#0f1318', // Card backgrounds
          850: '#141a21', // Elevated surfaces
          800: '#1a222c', // Borders, dividers
          700: '#2a3544', // Muted text
          600: '#3d4a5c', // Secondary text
          500: '#5a6a7f', // Tertiary elements
        },
        // Role-based Argonaut palette
        base: {
          bg: '#0d0f18',
          surface: '#12151e',
          elevated: '#181c27',
          border: '#1f2430',
        },
        text: {
          primary: '#fffaf3',
          secondary: '#c8c2b8',
          muted: '#6b6860',
          dim: '#4a4740',
        },
        interactive: {
          DEFAULT: '#00d7eb',
          hover: '#67ffef',
          muted: '#00d7eb40',
          dim: '#009aa8',
        },
        emphasis: {
          DEFAULT: '#8ce00a',
          dim: '#6ba808',
          glow: '#8ce00a30',
          bright: '#abe05a',
        },
        positive: {
          DEFAULT: '#8ce00a',
          dim: '#6ba808',
        },
        negative: {
          DEFAULT: '#ff000f',
          dim: '#cc000c',
          bright: '#ff273f',
        },
        warning: {
          DEFAULT: '#ffb900',
          dim: '#cc8c00',
          bright: '#ffd141',
        },
        info: {
          DEFAULT: '#008df8',
          dim: '#006bc0',
          bright: '#0092ff',
        },
      },
      fontFamily: {
        mono: ['var(--font-mono)', 'ui-monospace', 'monospace'],
        display: ['var(--font-display)', 'var(--font-mono)', 'monospace'], // For headers/titles
      },
      fontFeatureSettings: {
        tnum: '"tnum"',
      },
      boxShadow: {
        'terminal-glow': '0 0 6px #00ff4130, 0 0 12px #00ff4118',
        'terminal-glow-strong': '0 0 4px #00ff4160, 0 0 10px #00ff4130',
        'terminal-inset': 'inset 0 0 15px #00ff4108',
        'amber-glow': '0 0 6px #ffb00030, 0 0 12px #ffb00018',
        'amber-glow-strong': '0 0 4px #ffb00060, 0 0 10px #ffb00030',
        glow: '0 0 4px #8ce00a25, 0 0 10px #8ce00a15',
        'glow-strong': '0 0 3px #8ce00a50, 0 0 8px #8ce00a25',
        'glow-inset': 'inset 0 1px 4px #8ce00a10',
        'interactive-glow': '0 0 4px #00d7eb30, 0 0 10px #00d7eb18',
      },
      animation: {
        'terminal-flicker': 'terminal-flicker 0.15s infinite',
        'terminal-glow-pulse': 'terminal-glow-pulse 2s ease-in-out infinite',
        'digit-tick': 'digit-tick 0.2s ease-out',
        'scan-line': 'scan-line 8s linear infinite',
        'text-reveal': 'text-reveal 0.5s ease-out forwards',
        // Micro-interactions
        'glow-pulse': 'glow-pulse 1.5s ease-in-out infinite',
        'subtle-bounce': 'subtle-bounce 0.4s ease-out',
        glitch: 'glitch 0.3s ease-out',
      },
      keyframes: {
        'terminal-flicker': {
          '0%, 100%': { opacity: '1' },
          '50%': { opacity: '0.98' },
          '25%, 75%': { opacity: '0.96' },
        },
        'terminal-glow-pulse': {
          '0%, 100%': { filter: 'brightness(1)' },
          '50%': { filter: 'brightness(1.1)' },
        },
        'digit-tick': {
          '0%': { transform: 'translateY(-100%)', opacity: '0' },
          '50%': { transform: 'translateY(10%)', opacity: '1' },
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
        'subtle-bounce': {
          '0%': { transform: 'scale(1)' },
          '50%': { transform: 'scale(1.05)' },
          '100%': { transform: 'scale(1)' },
        },
        glitch: {
          '0%': { transform: 'translate(0)', opacity: '1' },
          '20%': { transform: 'translate(-2px, 2px)', opacity: '0.8' },
          '40%': { transform: 'translate(2px, -2px)', opacity: '0.9' },
          '60%': { transform: 'translate(-1px, 1px)', opacity: '0.8' },
          '80%': { transform: 'translate(1px, -1px)', opacity: '0.9' },
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
  plugins: [],
} satisfies Config;
