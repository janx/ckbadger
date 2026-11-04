import { MemoryRouter } from 'react-router-dom';
import { render as bareRender } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@/__tests__/utils/test-utils';
import { CrashPage } from '@/components/crash-page';
import { ErrorBoundary } from '@/components/error-boundary';
import { ErrorPageShell } from '@/components/error-page-shell';

vi.mock('@/src/navigation', async () => await vi.importActual('@/src/navigation'));

const HASH = '0xbbb655aefa778c0856e702030e9f8e9f6171daa392c90b3d9abe3d69c74b386b';
const ROUTE = ['all that is', 'was worked for.', 'this was not.'] as const;

describe('ErrorPageShell', () => {
  it('is the sign over the night: the thought as the h1 in the mono, the eyebrow and the path, no card', () => {
    const path = '/tx/' + 'a'.repeat(128);
    render(
      <ErrorPageShell status="Not found · 404" path={path} sign={ROUTE} contextSafe>
        <span>no page at this path.</span>
      </ErrorPageShell>
    );
    const sign = screen.getByRole('heading', {
      level: 1,
      name: 'all that is was worked for. this was not.',
    });
    expect(sign).toHaveClass('sign');
    expect(sign).not.toHaveAttribute('style');
    const tubes = sign.querySelectorAll('[data-tube]');
    expect(tubes).toHaveLength(3);
    expect(tubes[2]).toHaveClass('off');
    expect(screen.getByText('Not found · 404')).toHaveClass('eyebrow');
    expect(screen.getByText(path)).toHaveClass('break-all');
    expect(screen.getByText('no page at this path.').parentElement).toHaveClass('error-facts');
    expect(screen.getByRole('main').parentElement).toHaveClass('overflow-x-hidden');
    expect(screen.queryByRole('contentinfo')).toBeNull();
    // provider-safe: the static CSS night, no canvas, no realtime feed
    expect(screen.getByTestId('css-night')).toBeInTheDocument();
    expect(document.querySelector('canvas')).toBeNull();
    expect(document.querySelector('.error-night')).toHaveAttribute('data-night', 'static');
  });

  it('the eyebrow says what it means while the pointer is on it', () => {
    render(
      <ErrorPageShell status="Not found · 404" path="/nowhere" sign={ROUTE} contextSafe>
        <span>facts</span>
      </ErrorPageShell>
    );
    const eyebrow = screen.getByText('Not found · 404');
    fireEvent.pointerEnter(eyebrow);
    expect(eyebrow).toHaveTextContent('You are (not) here');
    fireEvent.pointerLeave(eyebrow);
    expect(eyebrow).toHaveTextContent('Not found · 404');
  });

  it('with providers it mounts the night; without WebGL2 (jsdom) the CSS night shows the page’s hash as 256 squares', () => {
    render(
      <MemoryRouter initialEntries={[`/mainnet/tx/${HASH}`]}>
        <ErrorPageShell status="Not found · 404" path={`/tx/${HASH}`} sign={ROUTE} hash={HASH}>
          <span>facts</span>
        </ErrorPageShell>
      </MemoryRouter>
    );
    expect(document.querySelector('.error-night')).toHaveAttribute('data-night', 'failed');
    expect(document.querySelector('canvas')).toHaveClass('hidden');
    const squares = screen.getByTestId('css-night').querySelectorAll('.error-night-bits i');
    expect(squares).toHaveLength(256);
    expect(squares[0]).toHaveClass('one');
    expect(squares[1]).toHaveClass('zero');
  });

  it('the crash page: the thought of the failed transition, the error in red, one footer, and no claim about the chain', () => {
    // drawn in place of the frame (the frame route's boundary): its own header and footer
    render(
      <MemoryRouter initialEntries={['/mainnet/fiber/channels/0x01']}>
        <CrashPage message="TypeError: missing channel output" standalone />
      </MemoryRouter>
    );
    expect(
      screen.getByRole('heading', { name: 'the chain keeps what succeeded. this did not.' })
    ).toHaveClass('sign-brownout');
    expect(screen.getByText('Error · not a 404')).toHaveClass('eyebrow');
    expect(screen.getByRole('alert')).toHaveTextContent('✗ TypeError: missing channel output');
    expect(screen.getAllByText('/mainnet/fiber/channels/0x01')).toHaveLength(2);
    expect(
      screen.getByText(
        'the chain was not checked in this view — this says nothing about what it holds'
      )
    ).toBeInTheDocument();
    expect(
      screen.queryByText(/nothing is wrong with the chain|reloading will not mend/i)
    ).toBeNull();
    expect(screen.getByRole('button', { name: '↻ try again' })).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: '→ copy details for a bug report' })
    ).toBeInTheDocument();
    expect(screen.getByRole('link', { name: '← Fiber channels' })).toHaveAttribute(
      'href',
      '/mainnet/fiber/channels'
    );
    expect(screen.getAllByRole('contentinfo')).toHaveLength(1);
    expect(document.querySelector('.error-night')).toHaveAttribute('data-night', 'failed');
  });

  // Visual finish V6: in the frame (the default) the frame draws the header and the footer; the
  // shell draws neither and fills what the header leaves (flex-1, not a whole screen).
  it('in the frame: no header and no footer of its own, and it fills what the header leaves', () => {
    render(
      <MemoryRouter initialEntries={['/mainnet/fiber/channels/0x01']}>
        <CrashPage message="TypeError: missing channel output" />
      </MemoryRouter>
    );
    expect(screen.getByRole('alert')).toHaveTextContent('✗ TypeError: missing channel output');
    expect(screen.queryByRole('banner')).toBeNull();
    expect(screen.queryByRole('contentinfo')).toBeNull();
    const shell = screen.getByRole('main').parentElement!;
    expect(shell).toHaveClass('flex-1');
    expect(shell).not.toHaveClass('min-h-screen');
  });

  it('drawn in place of the frame: its own header and footer, once each, a whole screen', () => {
    render(
      <MemoryRouter initialEntries={['/mainnet/fiber/channels/0x01']}>
        <CrashPage message="TypeError: missing channel output" standalone />
      </MemoryRouter>
    );
    expect(screen.getAllByRole('banner')).toHaveLength(1);
    expect(screen.getAllByRole('contentinfo')).toHaveLength(1);
    const shell = screen.getByRole('main').parentElement!;
    expect(shell).toHaveClass('min-h-screen');
    expect(shell).not.toHaveClass('flex-1');
  });

  it('renders a provider-safe fallback when the app itself throws', () => {
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {});
    function ThrowingChild(): never {
      throw new Error('block 12: invalid DAO field');
    }
    try {
      bareRender(
        <ErrorBoundary>
          <ThrowingChild />
        </ErrorBoundary>
      );
      expect(screen.getByRole('alert')).toHaveTextContent('block 12: invalid DAO field');
      expect(screen.getByRole('link', { name: '← back to the explorer' })).toHaveAttribute(
        'href',
        '/mainnet'
      );
      expect(screen.queryByRole('navigation', { name: 'Main navigation' })).toBeNull();
      expect(document.querySelector('canvas')).toBeNull();
      expect(document.querySelector('.error-night')).toHaveAttribute('data-night', 'static');
      expect(screen.getByRole('link', { name: 'CKBadger Home' })).toHaveAttribute('href', '/');
    } finally {
      consoleError.mockRestore();
    }
  });

  it('copies exact error context without inventing a chain tip', async () => {
    const oldClipboard = Object.getOwnPropertyDescriptor(navigator, 'clipboard');
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    });
    try {
      render(
        <MemoryRouter initialEntries={['/mainnet/tx/0xabc']}>
          <CrashPage message="block 12: invalid DAO field" />
        </MemoryRouter>
      );
      fireEvent.click(screen.getByRole('button', { name: '→ copy details for a bug report' }));
      await waitFor(() =>
        expect(within(screen.getByRole('main')).getByRole('status')).toHaveTextContent('copied')
      );
      expect(writeText).toHaveBeenCalledWith(
        expect.stringContaining('Error: block 12: invalid DAO field')
      );
      expect(writeText).toHaveBeenCalledWith(expect.stringContaining('Page: /mainnet/tx/0xabc'));
      expect(writeText).toHaveBeenCalledWith(expect.stringContaining('Network: mainnet'));
      expect(writeText).toHaveBeenCalledWith(
        expect.stringContaining('Chain tip: not checked in this error view')
      );
    } finally {
      if (oldClipboard) Object.defineProperty(navigator, 'clipboard', oldClipboard);
      else Reflect.deleteProperty(navigator, 'clipboard');
    }
  });
});
