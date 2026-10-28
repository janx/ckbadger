import { MemoryRouter } from 'react-router-dom';
import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@/__tests__/utils/test-utils';
import { CrashPage } from '@/components/crash-page';
import { ErrorBoundary } from '@/components/error-boundary';
import { ErrorPageShell } from '@/components/error-page-shell';

vi.mock('@/src/navigation', async () => await vi.importActual('@/src/navigation'));

describe('ErrorPageShell', () => {
  it('renders the self-hosted serif title card and wraps a long path at mobile width', () => {
    const path = '/tx/' + 'a'.repeat(128);
    render(
      <ErrorPageShell status="Not found · 404" path={path} title="An unanswered name." contextSafe>
        <p>Find another route.</p>
      </ErrorPageShell>
    );
    expect(screen.getByRole('heading', { name: 'An unanswered name.' })).toHaveStyle({
      fontFamily: 'var(--font-error-display)',
    });
    expect(screen.getByText(path)).toHaveClass('break-all');
    expect(screen.getByRole('main').parentElement).toHaveClass('overflow-x-hidden');
    expect(screen.queryByRole('contentinfo')).toBeNull();
  });

  it('shows crash context and one footer without claiming the chain is healthy', () => {
    render(
      <MemoryRouter initialEntries={['/mainnet/fiber/channels/0x01']}>
        <CrashPage message="TypeError: missing channel output" />
      </MemoryRouter>
    );
    expect(
      screen.getByRole('heading', { name: 'The page fell. The cause is unclear.' })
    ).toBeInTheDocument();
    expect(screen.getByRole('alert')).toHaveTextContent('TypeError: missing channel output');
    expect(screen.getAllByText('/fiber/channels/0x01')).toHaveLength(2);
    expect(
      screen.queryByText(/nothing is wrong with the chain|reloading will not mend/i)
    ).toBeNull();
    expect(screen.getByRole('button', { name: 'Try again →' })).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Copy details for a bug report' })
    ).toBeInTheDocument();
    expect(screen.getByText('Not checked in this error view')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Back to Fiber channels →' })).toHaveAttribute(
      'href',
      '/mainnet/fiber'
    );
    expect(screen.getAllByRole('contentinfo')).toHaveLength(1);
  });

  it('renders a provider-safe fallback when the app itself throws', () => {
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {});
    function ThrowingChild(): never {
      throw new Error('block 12: invalid DAO field');
    }
    try {
      render(
        <ErrorBoundary>
          <ThrowingChild />
        </ErrorBoundary>
      );
      expect(screen.getByRole('alert')).toHaveTextContent('block 12: invalid DAO field');
      expect(screen.getByRole('link', { name: 'Back to the explorer →' })).toHaveAttribute(
        'href',
        '/mainnet'
      );
      expect(screen.queryByRole('navigation', { name: 'Main navigation' })).toBeNull();
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
      fireEvent.click(screen.getByRole('button', { name: 'Copy details for a bug report' }));
      await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent('copied'));
      expect(writeText).toHaveBeenCalledWith(
        expect.stringContaining('Error: block 12: invalid DAO field')
      );
      expect(writeText).toHaveBeenCalledWith(expect.stringContaining('Page: /tx/0xabc'));
      expect(writeText).toHaveBeenCalledWith(
        expect.stringContaining('Chain tip: not checked in this error view')
      );
    } finally {
      if (oldClipboard) Object.defineProperty(navigator, 'clipboard', oldClipboard);
      else Reflect.deleteProperty(navigator, 'clipboard');
    }
  });
});
