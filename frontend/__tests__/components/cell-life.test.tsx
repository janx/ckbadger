import { describe, expect, it } from 'vitest';
import { render, screen, cleanup } from '@testing-library/react';
import { CellLife, CellLifePlaceholder } from '@/components/object/cell-life';

describe('CellLifePlaceholder', () => {
  it('renders a question mark', () => {
    render(<CellLifePlaceholder />);
    expect(screen.getByText('?')).toBeTruthy();
  });

  it('applies custom size', () => {
    const { container } = render(<CellLifePlaceholder size={100} />);
    const wrapper = container.firstElementChild as HTMLElement;
    expect(wrapper.style.width).toBe('100px');
    expect(wrapper.style.height).toBe('100px');
  });

  it('defaults to 56px', () => {
    const { container } = render(<CellLifePlaceholder />);
    const wrapper = container.firstElementChild as HTMLElement;
    expect(wrapper.style.width).toBe('56px');
    expect(wrapper.style.height).toBe('56px');
  });
});

describe('CellLife', () => {
  const testHash = '0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890';

  it('renders a canvas element', () => {
    const { container } = render(<CellLife hash={testHash} />);
    const canvas = container.querySelector('canvas');
    expect(canvas).not.toBeNull();
  });

  it('applies size to canvas style', () => {
    const { container } = render(<CellLife hash={testHash} size={100} />);
    const canvas = container.querySelector('canvas') as HTMLCanvasElement;
    expect(canvas.style.width).toBe('100px');
    expect(canvas.style.height).toBe('100px');
  });

  it('accepts isDualChain without error', () => {
    expect(() => {
      render(<CellLife hash={testHash} isDualChain />);
    }).not.toThrow();
  });

  it('renders wrapper with position:relative', () => {
    const { container } = render(<CellLife hash={testHash} />);
    const wrapper = container.firstElementChild as HTMLElement;
    expect(wrapper.style.position).toBe('relative');
  });

  it('cleans up on unmount without errors', () => {
    const { unmount } = render(<CellLife hash={testHash} />);
    expect(() => {
      unmount();
    }).not.toThrow();
  });

  it('defaults to 56px size', () => {
    const { container } = render(<CellLife hash={testHash} />);
    const canvas = container.querySelector('canvas') as HTMLCanvasElement;
    expect(canvas.style.width).toBe('56px');
    expect(canvas.style.height).toBe('56px');
  });
});
