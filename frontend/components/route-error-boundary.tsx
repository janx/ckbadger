'use client';

import { useRouteError, isRouteErrorResponse } from 'react-router-dom';
import { Header } from '@/components/layout/header';
import { TerminalPanel, TerminalPanelContent } from '@/components/ui/terminal-panel';

export function RouteErrorBoundary() {
  const error = useRouteError();

  let title = 'Something went wrong';
  let message = 'An unexpected error occurred while rendering this page.';

  if (isRouteErrorResponse(error)) {
    if (error.status === 404) {
      title = 'Page not found';
      message = 'The page you are looking for does not exist.';
    } else {
      title = `Error ${error.status}`;
      message = error.statusText || message;
    }
  } else if (error instanceof Error) {
    message = error.message;
  }

  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-4">
        <TerminalPanel>
          <TerminalPanelContent className="py-12 text-center">
            <h2 className="text-negative mb-2 text-xl">{title}</h2>
            <p className="text-text-dim mb-6 text-sm">{message}</p>
            <a
              href="/"
              className="bg-base-elevated hover:bg-base-surface border-base-border inline-block rounded border px-4 py-2 text-sm transition-colors"
            >
              Back to home
            </a>
          </TerminalPanelContent>
        </TerminalPanel>
      </main>
    </div>
  );
}
