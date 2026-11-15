'use client';

import { Component, ReactNode } from 'react';
import { CrashPage } from '@/components/crash-page';

interface Props {
  children: ReactNode;
  fallback?: ReactNode;
  /** Renders the caught error in the children's place; when given, it wins over `fallback`. */
  fallbackRender?: (error: Error) => ReactNode;
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
      if (this.props.fallbackRender) {
        return this.props.fallbackRender(this.state.error!);
      }

      if (this.props.fallback) {
        return this.props.fallback;
      }

      return (
        <CrashPage
          message={this.state.error?.message || 'An unexpected error occurred'}
          contextSafe
        />
      );
    }

    return this.props.children;
  }
}
