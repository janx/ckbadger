import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

// Rubbing × Song print, phase 2 (materials): the rules nothing rendered any more are gone from
// globals.css — the midnight CSS of glowing text, dots, lights, glitches and bars, the glowing
// terminal card the stats cards and the sync banner wore (now plain wells) and their keyframes.
// The CRT layers the atmosphere draws stay, and so does `.crt-screen` (pinned by redesign-tokens).

const css = readFileSync(resolve(__dirname, '../../app/globals.css'), 'utf-8');

describe('the skeleton’s CSS', () => {
  it('drops the rules nothing renders', () => {
    for (const gone of [
      '.scanline-overlay',
      '.terminal-text',
      '.terminal-text-dim',
      '.live-dot',
      '.indicator-light',
      '.glitch-hover',
      '.neon-bar-left',
      '.animate-blink-cursor',
      '@keyframes blink-cursor',
      '@keyframes scan-line',
      '@keyframes glitch',
      '@keyframes glow-pulse',
      '.terminal-card',
      '.terminal-border-glow',
    ])
      expect(css, gone).not.toContain(gone);
  });

  it('keeps the CRT layers and the pinned screen rule', () => {
    for (const kept of [
      '.crt-scanlines {',
      '.crt-vignette {',
      '.crt-grain {',
      '.crt-screen::before',
      '.row-scan {',
      '.phosphor-flare {',
      '.chart-line-halo {',
      '@keyframes live-pulse',
    ])
      expect(css, kept).toContain(kept);
  });
});
