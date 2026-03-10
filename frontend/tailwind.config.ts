import type { Config } from 'tailwindcss';

export default {
  content: ['./app/**/*.{js,ts,jsx,tsx,mdx}', './components/**/*.{js,ts,jsx,tsx,mdx}'],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        // 青绿暖白 palette (Qinglv Light)
        base: {
          bg: '#faf6f0', // 宣纸白
          surface: '#f3ede5', // 素纸
          elevated: '#efe8de', // 绢白
          border: '#e0d8cc', // 麻色
        },
        text: {
          primary: '#2a2520',
          secondary: '#7a7068',
          muted: '#b0a898',
          dim: '#d0c8bc',
        },
        interactive: {
          DEFAULT: '#3aaa80',
          hover: '#2cc878',
          muted: '#3aaa8020',
          dim: '#2d8a68',
        },
        emphasis: {
          DEFAULT: '#1e7a6a',
          dim: '#166858',
          glow: '#1e7a6a30',
          bright: '#28a088',
        },
        positive: {
          DEFAULT: '#4a8c5c', // 竹青
          dim: '#3a7048',
        },
        negative: {
          DEFAULT: '#c04040', // 朱红
          dim: '#a03535',
          bright: '#d45050',
        },
        warning: {
          DEFAULT: '#b88420', // 琥珀
          dim: '#9a6e1a',
          bright: '#d4a030',
        },
        info: {
          DEFAULT: '#3a6ea0', // 靛青
          dim: '#2e5a84',
          bright: '#4a88c0',
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
        glow: '0 0 6px #1e7a6a18, 0 0 14px #1e7a6a10',
        'glow-strong': '0 0 5px #1e7a6a28, 0 0 12px #1e7a6a18',
        'glow-inset': 'inset 0 1px 6px #1e7a6a08',
        'interactive-glow': '0 0 6px #3aaa8020, 0 0 14px #3aaa8010',
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
