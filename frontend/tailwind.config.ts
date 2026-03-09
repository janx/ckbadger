import type { Config } from 'tailwindcss';

export default {
  content: ['./app/**/*.{js,ts,jsx,tsx,mdx}', './components/**/*.{js,ts,jsx,tsx,mdx}'],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
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
