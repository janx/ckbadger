import type { Config } from 'tailwindcss';

export default {
  content: ['./app/**/*.{js,ts,jsx,tsx,mdx}', './components/**/*.{js,ts,jsx,tsx,mdx}'],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        // Citypop Midnight palette
        base: {
          bg: '#0c0a12',
          surface: '#110e1a',
          elevated: '#181424',
          border: '#1e1a2a',
        },
        text: {
          primary: '#f0e6ea',
          secondary: '#c0b0b8',
          muted: '#706068',
          dim: '#453d42',
        },
        interactive: {
          DEFAULT: '#4dd0c8',
          hover: '#78edd8',
          muted: '#4dd0c840',
          dim: '#38a89e',
        },
        emphasis: {
          DEFAULT: '#ff6b9d',
          dim: '#d4547e',
          glow: '#ff6b9d30',
          bright: '#ff8fb8',
        },
        positive: {
          DEFAULT: '#4dd0c8',
          dim: '#38a89e',
        },
        negative: {
          DEFAULT: '#ff4060',
          dim: '#cc3350',
          bright: '#ff6080',
        },
        warning: {
          DEFAULT: '#ff8c42',
          dim: '#cc7035',
          bright: '#ffb070',
        },
        info: {
          DEFAULT: '#64b5f6',
          dim: '#4a90c8',
          bright: '#90ccff',
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
        glow: '0 0 6px #ff6b9d20, 0 0 14px #ff6b9d10',
        'glow-strong': '0 0 5px #ff6b9d35, 0 0 12px #ff6b9d18',
        'glow-inset': 'inset 0 1px 6px #ff6b9d08',
        'interactive-glow': '0 0 6px #4dd0c820, 0 0 14px #4dd0c810',
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
