/**
 * The shared ErrorBoundary's `fallbackRender` (review 45fd ruling D7): it receives the caught error,
 * wins over `fallback`, and a new key gives the children a fresh start. `fallback` alone and the
 * default crash page are unchanged (the default is covered in error-page-shell.test.tsx).
 */
import { afterEach, beforeEach, describe, expect, it, vi, type MockInstance } from 'vitest';
import { render, screen } from '@testing-library/react';
import { ErrorBoundary } from '@/components/error-boundary';

function Thrower({ message }: { message: string }): never {
  throw new Error(message);
}

describe('ErrorBoundary fallbackRender', () => {
  // React and the boundary log every caught error; these errors are the point of the test.
  let quiet: MockInstance;
  beforeEach(() => {
    quiet = vi.spyOn(console, 'error').mockImplementation(() => {});
  });
  afterEach(() => {
    quiet.mockRestore();
  });

  it('renders the caught error in the children’s place', () => {
    const fallbackRender = vi.fn((error: Error) => <p>caught: {error.message}</p>);
    render(
      <ErrorBoundary fallbackRender={fallbackRender}>
        <Thrower message="boom" />
      </ErrorBoundary>
    );
    expect(screen.getByText('caught: boom')).toBeInTheDocument();
    expect(fallbackRender).toHaveBeenCalledWith(expect.objectContaining({ message: 'boom' }));
    expect(fallbackRender.mock.calls[0][0]).toBeInstanceOf(Error);
  });

  it('wins over fallback when both are given; fallback alone still works', () => {
    const { unmount } = render(
      <ErrorBoundary fallback={<p>static</p>} fallbackRender={(e) => <p>render {e.message}</p>}>
        <Thrower message="both" />
      </ErrorBoundary>
    );
    expect(screen.getByText('render both')).toBeInTheDocument();
    expect(screen.queryByText('static')).toBeNull();
    unmount();
    render(
      <ErrorBoundary fallback={<p>static</p>}>
        <Thrower message="alone" />
      </ErrorBoundary>
    );
    expect(screen.getByText('static')).toBeInTheDocument();
  });

  // B4 third round: a part starts again when what it draws changes, and a part that drew keeps
  // its elements (a key remounted them, and a page's table went stale under its reader)
  it('lets a caught error go when its reset key changes, and keeps children that drew', () => {
    const fallbackRender = (error: Error) => <p>caught: {error.message}</p>;
    const view = render(
      <ErrorBoundary resetKey="page 1" fallbackRender={fallbackRender}>
        <Thrower message="first" />
      </ErrorBoundary>
    );
    expect(screen.getByText('caught: first')).toBeInTheDocument();
    view.rerender(
      <ErrorBoundary resetKey="page 1" fallbackRender={fallbackRender}>
        <p>healthy</p>
      </ErrorBoundary>
    );
    expect(screen.getByText('caught: first')).toBeInTheDocument();
    view.rerender(
      <ErrorBoundary resetKey="page 2" fallbackRender={fallbackRender}>
        <p>healthy</p>
      </ErrorBoundary>
    );
    const drawn = screen.getByText('healthy');
    expect(screen.queryByText(/caught/)).toBeNull();
    view.rerender(
      <ErrorBoundary resetKey="page 3" fallbackRender={fallbackRender}>
        <p>healthy</p>
      </ErrorBoundary>
    );
    expect(screen.getByText('healthy')).toBe(drawn);
  });

  it('stays caught under the same key and starts afresh under a new one', () => {
    const fallbackRender = (error: Error) => <p>caught: {error.message}</p>;
    const view = render(
      <ErrorBoundary key="0xaa" fallbackRender={fallbackRender}>
        <Thrower message="first" />
      </ErrorBoundary>
    );
    expect(screen.getByText('caught: first')).toBeInTheDocument();
    view.rerender(
      <ErrorBoundary key="0xaa" fallbackRender={fallbackRender}>
        <p>healthy</p>
      </ErrorBoundary>
    );
    expect(screen.getByText('caught: first')).toBeInTheDocument();
    view.rerender(
      <ErrorBoundary key="0xbb" fallbackRender={fallbackRender}>
        <p>healthy</p>
      </ErrorBoundary>
    );
    expect(screen.getByText('healthy')).toBeInTheDocument();
    expect(screen.queryByText(/caught/)).toBeNull();
  });
});
