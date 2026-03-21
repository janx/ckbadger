import type { Config } from 'tailwindcss';

export default {
  content: ['./app/**/*.{js,ts,jsx,tsx,mdx}', './components/**/*.{js,ts,jsx,tsx,mdx}'],
  darkMode: 'class',
  theme: {
    container: {
      center: true,
      screens: {
        sm: '640px',
        md: '768px',
        lg: '1024px',
        xl: '1280px',
      },
    },
    extend: {
      colors: {
        // Chinese traditional palette — ink and silk in midnight
        base: {
          void: '#08090e',
          bg: '#0c0e15',
          surface: '#10131c',
          elevated: '#161a25',
          border: '#222840',
          'border-subtle': '#1a1f30',
        },
        text: {
          bright: '#dee2ec',
          DEFAULT: '#a0a8be',
          dim: '#606880',
          ghost: '#343c50',
        },
        interactive: {
          DEFAULT: '#68ccf0',
          hover: '#2edba3',
          muted: '#68ccf020',
          dim: '#4aa8d0',
        },
        emphasis: {
          DEFAULT: '#2edba3',
          dim: '#d0a840',
          glow: '#2edba340',
          bright: '#3ef0b8',
        },
        positive: {
          DEFAULT: '#2edba3',
          dim: '#1fb88a',
          bright: '#3ef0b8',
        },
        negative: {
          DEFAULT: '#e8555a',
          dim: '#c04048',
          bright: '#f06668',
        },
        warning: {
          DEFAULT: '#f2c55c',
          dim: '#d0a840',
          bright: '#f8d878',
        },
        info: {
          DEFAULT: '#68ccf0',
          dim: '#4aa8d0',
          bright: '#88ddf8',
        },
        // Chinese traditional named colors
        jade: {
          DEFAULT: '#2edba3',
          dim: '#1fb88a',
        },
        rouge: {
          DEFAULT: '#e8555a',
          dim: '#c04048',
        },
        aqua: {
          DEFAULT: '#68ccf0',
          dim: '#4aa8d0',
        },
        gold: {
          DEFAULT: '#f2c55c',
          dim: '#d0a840',
        },
        lavender: {
          DEFAULT: '#b8a9e8',
          dim: '#9888c8',
        },
        amber: {
          DEFAULT: '#d4883a',
          dim: '#b07028',
        },
        // Activity type semantic colors
        token: {
          DEFAULT: '#ff66aa',
          bright: '#ff88bb',
        },
        // Chart accent palette (12 colors)
        'accent-1': '#2edba3',
        'accent-2': '#e8555a',
        'accent-3': '#68ccf0',
        'accent-4': '#f2c55c',
        'accent-5': '#b8a9e8',
        'accent-6': '#d4883a',
        'accent-7': '#1fb88a',
        'accent-8': '#c04048',
        'accent-9': '#4aa8d0',
        'accent-10': '#d0a840',
        'accent-11': '#9888c8',
        'accent-12': '#b07028',
      },
      fontFamily: {
        mono: ['var(--font-mono)', 'ui-monospace', 'monospace'],
        display: ['var(--font-mono)', 'ui-monospace', 'monospace'],
      },
      fontFeatureSettings: {
        tnum: '"tnum"',
      },
      boxShadow: {
        glow: '0 0 8px #2edba320, 0 0 20px #2edba312',
        'glow-strong': '0 0 8px #2edba330, 0 0 20px #2edba320',
        'glow-inset': 'inset 0 1px 10px #2edba310',
        'glow-jade': '0 0 10px #2edba328, 0 0 20px #2edba314',
        'glow-rouge': '0 0 10px #e8555a28, 0 0 20px #e8555a14',
        'glow-aqua': '0 0 10px #68ccf028, 0 0 20px #68ccf014',
        'glow-gold': '0 0 10px #f2c55c28, 0 0 20px #f2c55c14',
        'glow-lavender': '0 0 10px #b8a9e828, 0 0 20px #b8a9e814',
        'glow-amber': '0 0 10px #d4883a28, 0 0 20px #d4883a14',
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
  plugins: [],
} satisfies Config;
