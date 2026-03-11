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
        <div className="border-negative/30 bg-negative/10 flex min-h-[200px] flex-col items-center justify-center rounded-lg border p-6">
          <div className="text-negative mb-2 text-xl">Something went wrong</div>
          <div className="text-text-dim mb-4 text-sm">
            {this.state.error?.message || 'An unexpected error occurred'}
          </div>
          <button
            onClick={() => this.setState({ hasError: false, error: undefined })}
            className="bg-negative hover:bg-negative-bright rounded px-4 py-2 text-sm text-white transition"
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
    <div className="border-negative/30 bg-negative/10 flex min-h-[200px] flex-col items-center justify-center rounded-lg border p-6">
      <div className="text-negative mb-2 text-xl">{title}</div>
      <div className="text-text-dim mb-4 text-sm">{message}</div>
      {onRetry && (
        <button
          onClick={onRetry}
          className="bg-negative hover:bg-negative-bright rounded px-4 py-2 text-sm text-white transition"
        >
          Try again
        </button>
      )}
    </div>
  );
}
