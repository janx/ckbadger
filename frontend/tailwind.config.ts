import type { Config } from 'tailwindcss';

export default {
  content: ['./app/**/*.{js,ts,jsx,tsx,mdx}', './components/**/*.{js,ts,jsx,tsx,mdx}'],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        // Midnight City Pop palette
        base: {
          bg: '#0c0e15',
          surface: '#10131c',
          elevated: '#161a25',
          border: '#1a1f30',
        },
        text: {
          primary: '#dee2ec',
          secondary: '#a0a8be',
          muted: '#606880',
          dim: '#343c50',
        },
        interactive: {
          DEFAULT: '#f0b866',
          hover: '#f8ca80',
          muted: '#f0b86620',
          dim: '#c89440',
        },
        emphasis: {
          DEFAULT: '#f0b866',
          dim: '#c89440',
          glow: '#f0b86630',
          bright: '#f8ca80',
        },
        positive: {
          DEFAULT: '#5ce0b8',
          dim: '#3cb898',
        },
        negative: {
          DEFAULT: '#e86080',
          dim: '#c04860',
          bright: '#f07898',
        },
        warning: {
          DEFAULT: '#f0b866',
          dim: '#c89440',
          bright: '#f8ca80',
        },
        info: {
          DEFAULT: '#6ab0e8',
          dim: '#4a88c0',
          bright: '#82c0f0',
        },
        // City pop accent colors (new)
        amber: {
          DEFAULT: '#f0b866',
          dim: '#c89440',
        },
        rose: {
          DEFAULT: '#e87ea0',
          dim: '#c0608a',
        },
        sky: {
          DEFAULT: '#6ab0e8',
          dim: '#4a88c0',
        },
        mint: {
          DEFAULT: '#5ce0b8',
          dim: '#3cb898',
        },
        violet: {
          DEFAULT: '#b08af0',
          dim: '#8a68c8',
        },
      },
      fontFamily: {
        mono: ['var(--font-mono)', 'ui-monospace', 'monospace'],
        display: ['var(--font-mono)', 'ui-monospace', 'monospace'],
      },
      fontFeatureSettings: {
        tnum: '"tnum"',
      },
      boxShadow: {
        glow: '0 0 8px #f0b86618, 0 0 16px #f0b86610',
        'glow-strong': '0 0 6px #f0b86628, 0 0 14px #f0b86618',
        'glow-inset': 'inset 0 1px 8px #f0b86608',
        'interactive-glow': '0 0 8px #f0b86620, 0 0 16px #f0b86610',
      },
      animation: {
        'terminal-flicker': 'terminal-flicker 0.15s infinite',
        'terminal-glow-pulse': 'terminal-glow-pulse 4s ease-in-out infinite',
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
          '50%': { opacity: '0.99' },
          '25%, 75%': { opacity: '0.98' },
        },
        'terminal-glow-pulse': {
          '0%, 100%': { filter: 'brightness(1)' },
          '50%': { filter: 'brightness(1.05)' },
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
  plugins: [],
} satisfies Config;
