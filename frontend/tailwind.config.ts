import type { Config } from 'tailwindcss';

export default {
  content: ['./app/**/*.{js,ts,jsx,tsx,mdx}', './components/**/*.{js,ts,jsx,tsx,mdx}'],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        // Midnight Neon palette — happy hacking in midnight
        base: {
          bg: '#0a0c14',
          surface: '#0e1119',
          elevated: '#141820',
          border: '#1c2236',
        },
        text: {
          primary: '#e4e8f4',
          secondary: '#b0b8d0',
          muted: '#6a7290',
          dim: '#3a4260',
        },
        interactive: {
          DEFAULT: '#ffcc44',
          hover: '#ffe066',
          muted: '#ffcc4420',
          dim: '#d4a020',
        },
        emphasis: {
          DEFAULT: '#ffcc44',
          dim: '#d4a020',
          glow: '#ffcc4440',
          bright: '#ffe066',
        },
        positive: {
          DEFAULT: '#00ffaa',
          dim: '#00cc88',
        },
        negative: {
          DEFAULT: '#ff4477',
          dim: '#cc3060',
          bright: '#ff6699',
        },
        warning: {
          DEFAULT: '#ffcc44',
          dim: '#d4a020',
          bright: '#ffe066',
        },
        info: {
          DEFAULT: '#44bbff',
          dim: '#2299dd',
          bright: '#66ddff',
        },
        // Neon accent colors
        amber: {
          DEFAULT: '#ffcc44',
          dim: '#d4a020',
        },
        rose: {
          DEFAULT: '#ff66aa',
          dim: '#dd4488',
        },
        sky: {
          DEFAULT: '#44bbff',
          dim: '#2299dd',
        },
        mint: {
          DEFAULT: '#00ffaa',
          dim: '#00cc88',
        },
        violet: {
          DEFAULT: '#bb88ff',
          dim: '#9966dd',
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
        glow: '0 0 8px #ffcc4420, 0 0 20px #ffcc4412',
        'glow-strong': '0 0 8px #ffcc4430, 0 0 20px #ffcc4420',
        'glow-inset': 'inset 0 1px 10px #ffcc4410',
        'interactive-glow': '0 0 10px #ffcc4428, 0 0 20px #ffcc4414',
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
