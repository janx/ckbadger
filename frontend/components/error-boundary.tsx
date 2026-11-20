'use client';

import { Component, ReactNode } from 'react';
import { CrashPage } from '@/components/crash-page';

interface Props {
  children: ReactNode;
  fallback?: ReactNode;
  /** Renders the caught error in the children's place; when given, it wins over `fallback`. */
  fallbackRender?: (error: Error) => ReactNode;
  /**
   * What the children draw (lane B4 third round): when it changes, a caught error is let go and the
   * children are drawn again; children that drew are not mounted anew (a `key` would remount them).
   */
  resetKey?: string;
}

interface State {
  hasError: boolean;
  error?: Error;
  resetKey?: string;
}

export class ErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = { hasError: false, resetKey: props.resetKey };
  }

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { hasError: true, error };
  }

  static getDerivedStateFromProps(props: Props, state: State): Partial<State> | null {
    return props.resetKey === state.resetKey
      ? null
      : { hasError: false, error: undefined, resetKey: props.resetKey };
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
