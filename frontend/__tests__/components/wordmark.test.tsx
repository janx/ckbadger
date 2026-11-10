import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { render as bareRender } from '@testing-library/react';
import { render, screen } from '../utils/test-utils';
import { Wordmark } from '@/components/layout/wordmark';

// Rubbing × Song print, phase 2 (boards/wordmark.js, variant 2): the logo is the cinnabar prompt
// square, then the name in lowercase, JetBrains Mono 600 at 15 px in the moon ink, 10 px apart.
// The webp badge, its tilt, halo and per-block flicker are gone.

const css = readFileSync(resolve(__dirname, '../../app/globals.css'), 'utf-8');

describe('Wordmark', () => {
  it('is a home link holding the prompt square and the lowercase name, no image', () => {
    render(<Wordmark />);
    const link = screen.getByRole('link', { name: 'CKBadger home' });
    expect(link).toHaveAttribute('href', '/mainnet');
    expect(link).toHaveTextContent(/^ckbadger$/);
    expect(link).toHaveClass(
      'flex',
      'items-center',
      'gap-2.5',
      'font-semibold',
      'text-text-bright'
    );
    expect(link.querySelector('img')).toBeNull();
    expect(link.querySelector('svg')).toBeNull();
    const [square, name] = Array.from(link.children);
    expect(square).toHaveClass('prompt-square');
    expect(square).toHaveAttribute('aria-hidden', 'true');
    expect(square).toBeEmptyDOMElement();
    expect(name).toHaveClass('wordmark');
    expect(name).toHaveTextContent('ckbadger');
  });

  it('draws a bare home link without the router, for the error page that has none', () => {
    bareRender(<Wordmark plain />);
    const link = screen.getByRole('link', { name: 'CKBadger home' });
    expect(link).toHaveAttribute('href', '/');
    expect(link.querySelector('.prompt-square')).not.toBeNull();
    expect(link).toHaveTextContent(/^ckbadger$/);
  });

  it('paints the square 8 px in the cinnabar mark, square-cornered and unlit, the name at 15 px', () => {
    const square = css.match(/\.prompt-square[^{]*\{([^}]*)\}/)!;
    expect(square[1]).toContain('width: 8px;');
    expect(square[1]).toContain('height: 8px;');
    expect(square[1]).toContain('background: var(--c-mark);');
    expect(square[1]).not.toMatch(/border-radius|box-shadow/);
    expect(css).toMatch(/\.wordmark\s*\{\s*font-size: 15px;/);
  });

  it('leaves no trace of the badge in the styles', () => {
    expect(css).not.toMatch(/\.logo-image|logo-breathe|neon-flicker/);
  });
});
