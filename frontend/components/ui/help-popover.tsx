'use client';

import { type CSSProperties, type ReactNode, useEffect, useId, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { cn } from '@/lib/utils';

interface HelpPopoverProps {
  label: string;
  title?: string;
  children: ReactNode;
  align?: 'start' | 'end';
  className?: string;
  contentClassName?: string;
}

const HOVER_CLOSE_DELAY_MS = 120;

export function HelpPopover({
  label,
  title,
  children,
  align = 'start',
  className,
  contentClassName,
}: HelpPopoverProps) {
  const [hoveredTrigger, setHoveredTrigger] = useState(false);
  const [hoveredPanel, setHoveredPanel] = useState(false);
  const [focused, setFocused] = useState(false);
  const [pinned, setPinned] = useState(false);
  const [position, setPosition] = useState<CSSProperties | null>(null);
  const wrapperRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const hoverCloseTimerRef = useRef<number | null>(null);
  const panelId = useId();
  const isOpen = hoveredTrigger || hoveredPanel || focused || pinned;

  const clearHoverCloseTimer = () => {
    if (hoverCloseTimerRef.current !== null) {
      window.clearTimeout(hoverCloseTimerRef.current);
      hoverCloseTimerRef.current = null;
    }
  };

  const scheduleHoverClose = () => {
    clearHoverCloseTimer();
    hoverCloseTimerRef.current = window.setTimeout(() => {
      setHoveredTrigger(false);
      setHoveredPanel(false);
      hoverCloseTimerRef.current = null;
    }, HOVER_CLOSE_DELAY_MS);
  };

  useEffect(() => {
    const handlePointerDown = (event: MouseEvent | TouchEvent) => {
      const target = event.target as Node;
      const clickedTrigger = triggerRef.current?.contains(target) ?? false;
      const clickedPanel = panelRef.current?.contains(target) ?? false;
      if (!clickedTrigger && !clickedPanel) {
        clearHoverCloseTimer();
        setPinned(false);
        setHoveredTrigger(false);
        setHoveredPanel(false);
        setFocused(false);
      }
    };

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        clearHoverCloseTimer();
        setPinned(false);
        setHoveredTrigger(false);
        setHoveredPanel(false);
        setFocused(false);
      }
    };

    document.addEventListener('mousedown', handlePointerDown);
    document.addEventListener('touchstart', handlePointerDown);
    document.addEventListener('keydown', handleKeyDown);

    return () => {
      document.removeEventListener('mousedown', handlePointerDown);
      document.removeEventListener('touchstart', handlePointerDown);
      document.removeEventListener('keydown', handleKeyDown);
      clearHoverCloseTimer();
    };
  }, []);

  useEffect(() => {
    if (!isOpen || typeof window === 'undefined') {
      return;
    }

    const updatePosition = () => {
      const triggerRect = triggerRef.current?.getBoundingClientRect();
      if (!triggerRect) {
        return;
      }

      const panelWidth = panelRef.current?.offsetWidth ?? 352;
      const panelHeight = panelRef.current?.offsetHeight ?? 180;
      const viewportWidth = window.innerWidth;
      const viewportHeight = window.innerHeight;
      const edgePadding = 8;
      const gap = 8;

      const maxLeft = Math.max(edgePadding, viewportWidth - panelWidth - edgePadding);
      const preferredLeft = align === 'end' ? triggerRect.right - panelWidth : triggerRect.left;
      const left = Math.min(Math.max(edgePadding, preferredLeft), maxLeft);

      const preferredTop = triggerRect.bottom + gap;
      const canFlipAbove = triggerRect.top - gap - panelHeight >= edgePadding;
      const top =
        preferredTop + panelHeight <= viewportHeight - edgePadding || !canFlipAbove
          ? preferredTop
          : triggerRect.top - panelHeight - gap;

      setPosition({
        left,
        top,
      });
    };

    updatePosition();
    window.addEventListener('resize', updatePosition);
    window.addEventListener('scroll', updatePosition, true);

    return () => {
      window.removeEventListener('resize', updatePosition);
      window.removeEventListener('scroll', updatePosition, true);
    };
  }, [align, isOpen]);

  const popover =
    isOpen && typeof document !== 'undefined'
      ? createPortal(
          <div
            id={panelId}
            ref={panelRef}
            role="dialog"
            aria-label={title ?? label}
            className={cn(
              'border-base-border/80 bg-base-surface/95 text-text-dim fixed z-[10000] w-[22rem] max-w-[min(24rem,calc(100vw-1rem))] rounded-lg border p-3 shadow-2xl backdrop-blur-sm',
              contentClassName
            )}
            style={position ?? undefined}
            onMouseEnter={() => {
              clearHoverCloseTimer();
              setHoveredPanel(true);
            }}
            onMouseLeave={() => {
              scheduleHoverClose();
            }}
            onFocus={() => setFocused(true)}
            onBlur={(event) => {
              const nextTarget = event.relatedTarget as Node | null;
              const movingIntoTrigger = triggerRef.current?.contains(nextTarget) ?? false;
              const movingIntoPanel = panelRef.current?.contains(nextTarget) ?? false;
              if (!movingIntoTrigger && !movingIntoPanel) {
                clearHoverCloseTimer();
                setFocused(false);
                setPinned(false);
              }
            }}
          >
            {title && (
              <div className="text-text-dim border-base-border/50 mb-2 border-b pb-2 font-mono text-[11px] uppercase tracking-wider">
                {title}
              </div>
            )}
            <div className="space-y-2 text-xs">{children}</div>
          </div>,
          document.body
        )
      : null;

  return (
    <div
      ref={wrapperRef}
      className={cn('inline-flex items-center', className)}
      onMouseEnter={() => {
        clearHoverCloseTimer();
        setHoveredTrigger(true);
      }}
      onMouseLeave={() => {
        scheduleHoverClose();
      }}
      onFocus={() => setFocused(true)}
      onBlur={(event) => {
        const nextTarget = event.relatedTarget as Node | null;
        const movingIntoTrigger = triggerRef.current?.contains(nextTarget) ?? false;
        const movingIntoPanel = panelRef.current?.contains(nextTarget) ?? false;
        if (!movingIntoTrigger && !movingIntoPanel) {
          clearHoverCloseTimer();
          setFocused(false);
          setPinned(false);
        }
      }}
    >
      <button
        ref={triggerRef}
        type="button"
        aria-label={label}
        aria-expanded={isOpen}
        aria-controls={panelId}
        className={cn(
          'text-text-dim border-base-border/70 hover:border-emphasis/40 hover:text-emphasis',
          'relative inline-flex h-5 w-5 items-center justify-center rounded-full border',
          'bg-base-elevated/70 font-mono text-[10px] transition-colors',
          'before:absolute before:-inset-3 before:content-[""]'
        )}
        onClick={(event) => {
          event.preventDefault();
          event.stopPropagation();
          setPinned((current) => !current);
        }}
      >
        ?
      </button>
      {popover}
    </div>
  );
}
