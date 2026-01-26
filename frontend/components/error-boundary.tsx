'use client';

import { Component, ReactNode } from 'react';

interface Props {
  children: ReactNode;
  fallback?: ReactNode;
}

interface State {
  hasError: boolean;
  error?: Error;
}

export class ErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = { hasError: false };
  }

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, errorInfo: React.ErrorInfo) {
    console.error('Error caught by boundary:', error, errorInfo);
  }

  render() {
    if (this.state.hasError) {
      if (this.props.fallback) {
        return this.props.fallback;
      }

      return (
        <div className="flex min-h-[200px] flex-col items-center justify-center rounded-lg border border-red-800 bg-red-900/20 p-6">
          <div className="mb-2 text-xl text-red-400">Something went wrong</div>
          <div className="mb-4 text-sm text-slate-400">
            {this.state.error?.message || 'An unexpected error occurred'}
          </div>
          <button
            onClick={() => this.setState({ hasError: false, error: undefined })}
            className="rounded bg-red-600 px-4 py-2 text-sm text-white transition hover:bg-red-700"
          >
            Try again
          </button>
        </div>
      );
    }

    return this.props.children;
  }
}

interface ErrorFallbackProps {
  title?: string;
  message?: string;
  onRetry?: () => void;
}

export function ErrorFallback({
  title = 'Something went wrong',
  message = 'An unexpected error occurred',
  onRetry,
}: ErrorFallbackProps) {
  return (
    <div className="flex min-h-[200px] flex-col items-center justify-center rounded-lg border border-red-800 bg-red-900/20 p-6">
      <div className="mb-2 text-xl text-red-400">{title}</div>
      <div className="mb-4 text-sm text-slate-400">{message}</div>
      {onRetry && (
        <button
          onClick={onRetry}
          className="rounded bg-red-600 px-4 py-2 text-sm text-white transition hover:bg-red-700"
        >
          Try again
        </button>
      )}
    </div>
  );
}
