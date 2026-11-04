'use client';

import { type HTMLAttributes, type ReactNode } from 'react';
import { cn } from '@/lib/utils';

type IndicatorStatus = 'active' | 'warning' | 'inactive' | 'none';
type AccentColor = 'jade' | 'aqua' | 'gold' | 'rouge' | 'lavender' | 'amber';

/**
 * The frame's top edge colour (`--neon-color`, read by `.neon-edge-top::after`), as a class rather
 * than an inline style: the frame then carries no inline ink, which the pages' "no coloured text
 * outside the graphics" checks count. Jade is the approved .45; the others keep their .4.
 */
const accentClass: Record<AccentColor, string> = {
  jade: '[--neon-color:rgba(46,219,163,0.45)]',
  aqua: '[--neon-color:rgba(104,204,240,0.4)]',
  gold: '[--neon-color:rgba(242,197,92,0.4)]',
  rouge: '[--neon-color:rgba(232,85,90,0.4)]',
  lavender: '[--neon-color:rgba(184,169,232,0.4)]',
  amber: '[--neon-color:rgba(212,136,58,0.4)]',
};

interface TerminalPanelProps extends HTMLAttributes<HTMLDivElement> {
  children: ReactNode;
  variant?: 'default' | 'elevated' | 'inset';
  glow?: boolean;
  accentColor?: AccentColor;
}

/**
 * The one instrument frame (Midnight, continued, move 3: "instruments are framed, text is flat"):
 * a #222840 hairline, an 8 px radius, glass (material pass v2, move 2: `.glass` — 72 % opaque over
 * the night sky, a 1 px highlight on the top edge, a dark line on the bottom edge; no
 * backdrop-filter, see globals.css), a jade edge along the top (`.neon-edge-top`) and inset
 * scanlines. The transaction pipeline, the tx page's cells diagram, the cell page's byte dump,
 * the DNA gel and the network map sit in it; prose, tables and charts stay unframed.
 */
export function TerminalPanel({
  children,
  className,
  variant = 'default',
  glow = false,
  accentColor = 'jade',
  style,
  ...props
}: TerminalPanelProps) {
  const variantClasses = {
    // Glass frames take the glass radius (visual finish V10: `rounded-lg`, 8 px); the flat
    // variants keep the default 4 px.
    default: 'glass border-base-border rounded-lg',
    elevated: 'bg-base-elevated border-base-border rounded',
    inset: 'bg-base-bg border-base-border shadow-glow-inset rounded',
  };

  return (
    <div
      {...props}
      data-instrument-frame=""
      className={cn(
        'neon-edge-top relative overflow-hidden border transition-shadow duration-300',
        accentClass[accentColor],
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
              'repeating-linear-gradient(0deg, rgba(0,0,0,0.03) 0px, rgba(0,0,0,0.03) 1px, transparent 1px, transparent 2px)',
          }}
        />
      </div>

      <div className="relative z-10">{children}</div>
    </div>
  );
}

interface TerminalPanelHeaderProps {
  children: ReactNode;
  className?: string;
  indicator?: IndicatorStatus;
  actions?: ReactNode;
}

/** The frame's eyebrow: the section mark, then the instrument's name in upper case (12 px,
 * `.eyebrow`: 500 at .12 em — material pass v2, move 3). */
const MARK_CLASS: Record<IndicatorStatus, string> = {
  // the mark as it is on a tier-1 h2
  none: 'section-mark',
  // a realtime feed: the mark breathes
  active: 'section-mark section-mark-live',
  warning: 'section-mark section-mark-gold',
  inactive: 'section-mark opacity-30',
};

export function TerminalPanelHeader({
  children,
  className,
  indicator = 'none',
  actions,
}: TerminalPanelHeaderProps) {
  return (
    <div
      className={cn(
        'flex flex-wrap items-start justify-between gap-3 px-3 py-2 sm:items-center',
        'border-base-border border-b',
        'from-base-elevated/60 bg-gradient-to-r to-transparent',
        className
      )}
    >
      <div className="flex min-w-0 items-center gap-2.5">
        <span
          aria-hidden="true"
          data-indicator={indicator}
          data-testid="terminal-panel-mark"
          className={MARK_CLASS[indicator]}
        />
        <div className="eyebrow text-text-dim text-small min-w-0 break-words font-mono">
          {children}
        </div>
      </div>

      {actions && (
        <div className="flex max-w-full flex-wrap items-center justify-end gap-2">{actions}</div>
      )}
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

interface TerminalPanelFooterProps {
  children: ReactNode;
  className?: string;
}

export function TerminalPanelFooter({ children, className }: TerminalPanelFooterProps) {
  return (
    <div
      className={cn(
        'border-base-border border-t px-3 py-2',
        'to-base-elevated/30 bg-gradient-to-r from-transparent',
        className
      )}
    >
      {children}
    </div>
  );
}

interface TerminalDividerProps {
  className?: string;
  label?: string;
}

export function TerminalDivider({ className, label }: TerminalDividerProps) {
  if (label) {
    return (
      <div className={cn('flex items-center gap-3 py-2', className)}>
        <div className="via-base-border h-px flex-1 bg-gradient-to-r from-transparent to-transparent" />
        <span className="eyebrow text-text-dim text-small font-mono">{label}</span>
        <div className="via-base-border h-px flex-1 bg-gradient-to-r from-transparent to-transparent" />
      </div>
    );
  }

  return (
    <div
      className={cn(
        'via-base-border h-px bg-gradient-to-r from-transparent to-transparent',
        className
      )}
    />
  );
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
        'border-base-border-subtle border-b px-3 py-2 last:border-b-0',
        hoverable && 'row-scan hover:bg-base-elevated/50 transition-colors',
        className
      )}
    >
      {children}
    </div>
  );
}
