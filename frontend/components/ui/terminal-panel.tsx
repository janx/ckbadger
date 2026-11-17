'use client';

import { type HTMLAttributes, type ReactNode } from 'react';
import { cn } from '@/lib/utils';

interface TerminalPanelProps extends HTMLAttributes<HTMLDivElement> {
  children: ReactNode;
  variant?: 'default' | 'elevated' | 'inset';
  glow?: boolean;
}

/**
 * The one instrument frame (Midnight, continued, move 3: "instruments are framed, text is flat"),
 * a flat tile since rubbing × Song print: the surface under its own 1 px border in the line ink,
 * square corners, `.glass` (its materials now the opaque surface with no sheen, no edges, no ring;
 * see globals.css) and inset scanlines — no lit edge along the top. The tx page's two sheets of
 * cells (one frame each, the seam between), the cell page's byte dump, the DNA gel and the network
 * map sit in it; prose, tables, charts and the home page's pipeline stay unframed.
 */
export function TerminalPanel({
  children,
  className,
  variant = 'default',
  glow = false,
  style,
  ...props
}: TerminalPanelProps) {
  const variantClasses = {
    default: 'glass border-base-border',
    elevated: 'bg-base-elevated border-base-border',
    inset: 'bg-base-bg border-base-border shadow-glow-inset',
  };

  return (
    <div
      {...props}
      data-instrument-frame=""
      className={cn(
        'relative overflow-hidden border transition-shadow duration-300',
        variantClasses[variant],
        glow && 'hover:shadow-glow',
        className
      )}
      style={style}
    >
      <div className="pointer-events-none absolute inset-0">
        <div
          className="absolute inset-0"
          style={{
            background:
              'repeating-linear-gradient(0deg, color-mix(in srgb, var(--m-inset-scan) 50%, transparent) 0px, color-mix(in srgb, var(--m-inset-scan) 50%, transparent) 1px, transparent 1px, transparent 2px)',
          }}
        />
      </div>

      <div className="relative z-10">{children}</div>
    </div>
  );
}

interface TerminalPanelContentProps {
  children: ReactNode;
  className?: string;
  padding?: 'none' | 'sm' | 'md' | 'lg';
}

export function TerminalPanelContent({
  children,
  className,
  padding = 'md',
}: TerminalPanelContentProps) {
  const paddingClasses = {
    none: '',
    sm: 'p-2',
    md: 'p-3',
    lg: 'p-4',
  };

  return <div className={cn(paddingClasses[padding], className)}>{children}</div>;
}

interface TerminalRowProps extends HTMLAttributes<HTMLDivElement> {
  children: ReactNode;
  className?: string;
  hoverable?: boolean;
}

export function TerminalRow({ children, className, hoverable = true, ...props }: TerminalRowProps) {
  return (
    <div
      {...props}
      className={cn(
        // a row rule is the subtle ink, #1a1f30 (material pass v2, move 6)
        'border-row-rule border-b px-3 py-2 last:border-b-0',
        hoverable && 'row-scan hover:bg-base-elevated/50 transition-colors',
        className
      )}
    >
      {children}
    </div>
  );
}
