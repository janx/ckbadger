'use client';

import { useMemo, useRef, useState, useEffect, useCallback } from 'react';
import { createPortal } from 'react-dom';
import Link from '@/components/ui/link';
import { cn } from '@/lib/utils';

export type TxCategory = 'normal' | 'cellbase' | 'dao';

export interface TxItem {
  id: string;
  size: number;
  fee?: number;
  feeRate?: number;
  category: TxCategory;
}

interface PackedContainerProps {
  title: string;
  subtitle?: string;
  type: 'mempool' | 'proposals' | 'tip';
  items: TxItem[];
  totalCount: number;
  blockNumber?: number;
  emptyText?: string;
  globalMaxSize: number;
}

const TYPE_CONFIG = {
  mempool: {
    borderColor: 'border-base-border/50',
    bgGradient: 'from-base-elevated/50 to-base-surface/50',
    titleColor: 'text-text-secondary',
    countColor: 'text-text-muted',
  },
  proposals: {
    borderColor: 'border-warning-600/50',
    bgGradient: 'from-warning-950/30 to-base-surface/50',
    titleColor: 'text-warning-400',
    countColor: 'text-warning-500/80',
  },
  tip: {
    borderColor: 'border-emphasis/50',
    bgGradient: 'from-emphasis-dim/20 to-base-surface/50',
    titleColor: 'text-emphasis',
    countColor: 'text-emphasis-dim',
  },
};

const CATEGORY_COLORS: Record<TxCategory, Record<'mempool' | 'proposals' | 'tip', string>> = {
  normal: {
    mempool: 'bg-base-border/80 hover:bg-base-border',
    proposals: 'bg-warning-600/80 hover:bg-warning',
    tip: 'bg-emphasis/80 hover:bg-emphasis-dim',
  },
  cellbase: {
    mempool: 'bg-emerald-600/80 hover:bg-emerald-500',
    proposals: 'bg-emerald-600/80 hover:bg-emerald-500',
    tip: 'bg-emerald-600/80 hover:bg-emerald-500',
  },
  dao: {
    mempool: 'bg-warning/80 hover:bg-warning-400',
    proposals: 'bg-warning/80 hover:bg-warning-400',
    tip: 'bg-warning/80 hover:bg-warning-400',
  },
};

const CATEGORY_LABELS: Record<TxCategory, string> = {
  normal: 'Transaction',
  cellbase: 'Cellbase (Mining Reward)',
  dao: 'Nervos DAO',
};

function formatBytes(bytes: number): string {
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${bytes} B`;
}

function formatFee(shannons: number): string {
  const ckb = shannons / 100_000_000;
  if (ckb >= 1) return `${ckb.toFixed(4)} CKB`;
  return `${shannons.toLocaleString()} shannons`;
}

function truncateHash(hash: string): string {
  if (hash.length <= 16) return hash;
  return `${hash.slice(0, 8)}...${hash.slice(-6)}`;
}

const MIN_BOX_SIZE = 6;
const MAX_BOX_SIZE = 40;
const BOX_GAP = 1;

function calculateBoxSize(size: number, maxSize: number): number {
  const normalized = Math.sqrt(size / maxSize);
  return Math.max(
    MIN_BOX_SIZE,
    Math.min(MAX_BOX_SIZE, MIN_BOX_SIZE + normalized * (MAX_BOX_SIZE - MIN_BOX_SIZE))
  );
}

interface PackedBox {
  item: TxItem;
  size: number;
  x: number;
  y: number;
}

function packBoxes(
  items: TxItem[],
  containerWidth: number,
  globalMaxSize: number
): { boxes: PackedBox[]; height: number } {
  if (items.length === 0 || containerWidth <= 0) {
    return { boxes: [], height: 0 };
  }

  const sortedItems = [...items].sort((a, b) => b.size - a.size);

  const boxes: PackedBox[] = [];
  let currentX = 0;
  let currentY = 0;
  let rowHeight = 0;

  for (const item of sortedItems) {
    const boxSize = calculateBoxSize(item.size, globalMaxSize);

    if (currentX + boxSize > containerWidth && currentX > 0) {
      currentX = 0;
      currentY += rowHeight + BOX_GAP;
      rowHeight = 0;
    }

    boxes.push({
      item,
      size: boxSize,
      x: currentX,
      y: currentY,
    });

    currentX += boxSize + BOX_GAP;
    rowHeight = Math.max(rowHeight, boxSize);
  }

  return { boxes, height: currentY + rowHeight };
}

interface TxBoxProps {
  item: TxItem;
  boxSize: number;
  x: number;
  y: number;
  type: 'mempool' | 'proposals' | 'tip';
  isCommitted?: boolean;
}

function TxBox({ item, boxSize, x, y, type, isCommitted }: TxBoxProps) {
  const [showTooltip, setShowTooltip] = useState(false);
  const [tooltipPos, setTooltipPos] = useState({ x: 0, y: 0 });
  const boxRef = useRef<HTMLDivElement>(null);

  const updateTooltipPos = useCallback(() => {
    if (boxRef.current) {
      const rect = boxRef.current.getBoundingClientRect();
      setTooltipPos({
        x: rect.left + rect.width / 2,
        y: rect.top,
      });
    }
  }, []);

  useEffect(() => {
    if (showTooltip) {
      updateTooltipPos();
    }
  }, [showTooltip, updateTooltipPos]);

  const handleClick = (e: React.MouseEvent) => {
    if (isCommitted) {
      e.preventDefault();
      e.stopPropagation();
      window.location.href = `/tx/${item.id}`;
    }
  };

  return (
    <>
      <div
        ref={boxRef}
        className={cn(
          'absolute cursor-pointer rounded-[2px] border border-black/30 transition-colors duration-100',
          CATEGORY_COLORS[item.category][type]
        )}
        style={{
          left: x,
          top: y,
          width: boxSize,
          height: boxSize,
        }}
        onMouseEnter={() => setShowTooltip(true)}
        onMouseLeave={() => setShowTooltip(false)}
        onClick={handleClick}
      />
      {showTooltip &&
        typeof window !== 'undefined' &&
        createPortal(
          <div
            className="border-base-border/50 bg-base-surface/95 pointer-events-none fixed z-[9999] -translate-x-1/2 -translate-y-full whitespace-nowrap rounded-lg border px-3 py-2 text-xs shadow-xl backdrop-blur-sm"
            style={{
              left: tooltipPos.x,
              top: tooltipPos.y - 8,
            }}
          >
            <div className="text-text-muted mb-1 text-[10px]">{CATEGORY_LABELS[item.category]}</div>
            <div className="text-text-muted space-y-0.5">
              <div>
                TX: <span className="font-mono text-white">{truncateHash(item.id)}</span>
              </div>
              {item.size > 0 && (
                <div>
                  Size: <span className="text-white">{formatBytes(item.size)}</span>
                </div>
              )}
              {item.fee !== undefined && item.fee > 0 && (
                <div>
                  Fee: <span className="text-white">{formatFee(item.fee)}</span>
                </div>
              )}
              {item.feeRate !== undefined && item.feeRate > 0 && (
                <div>
                  Fee Rate: <span className="text-white">{item.feeRate.toFixed(2)} sh/B</span>
                </div>
              )}
            </div>
            <div className="border-t-base-surface/95 absolute left-1/2 top-full -translate-x-1/2 border-4 border-transparent" />
          </div>,
          document.body
        )}
    </>
  );
}

export function PackedContainer({
  title,
  subtitle,
  type,
  items,
  totalCount,
  blockNumber,
  emptyText = 'No transactions',
  globalMaxSize,
}: PackedContainerProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [containerWidth, setContainerWidth] = useState(0);
  const config = TYPE_CONFIG[type];

  useEffect(() => {
    if (!containerRef.current) return;

    const updateWidth = () => {
      if (containerRef.current) {
        setContainerWidth(containerRef.current.clientWidth);
      }
    };

    updateWidth();

    const observer = new ResizeObserver(updateWidth);
    observer.observe(containerRef.current);
    return () => observer.disconnect();
  }, []);

  const { boxes } = useMemo(() => {
    return packBoxes(items, containerWidth, globalMaxSize);
  }, [items, containerWidth, globalMaxSize]);

  const content = (
    <div
      className={cn(
        'flex h-full min-h-[180px] flex-col rounded-xl border-2 bg-gradient-to-br p-3 transition-all duration-200 sm:min-h-[220px] sm:p-4',
        config.borderColor,
        config.bgGradient,
        type === 'tip' &&
          blockNumber &&
          'hover:border-emphasis/70 hover:shadow-glow cursor-pointer hover:shadow-lg'
      )}
    >
      <div className="mb-2 flex items-center justify-between sm:mb-3">
        <div>
          <h3 className={cn('text-sm font-bold sm:text-base', config.titleColor)}>{title}</h3>
          {subtitle && <div className="text-text-muted text-[10px] sm:text-xs">{subtitle}</div>}
        </div>
        <div className={cn('text-right text-xs sm:text-sm', config.countColor)}>
          <span className="font-bold text-white">{totalCount.toLocaleString()}</span>
          <span className="ml-1">txs</span>
        </div>
      </div>

      <div ref={containerRef} className="relative flex-1 overflow-hidden rounded-lg bg-black/20">
        {items.length === 0 ? (
          <div className="text-text-muted flex h-full w-full items-center justify-center text-xs">
            {emptyText}
          </div>
        ) : (
          boxes.map((box, idx) => (
            <TxBox
              key={box.item.id || idx}
              item={box.item}
              boxSize={box.size}
              x={box.x}
              y={box.y}
              type={type}
              isCommitted={type === 'tip'}
            />
          ))
        )}
      </div>
    </div>
  );

  if (type === 'tip' && blockNumber) {
    return (
      <Link href={`/blocks/${blockNumber}`} className="block flex-1">
        {content}
      </Link>
    );
  }

  return <div className="flex-1">{content}</div>;
}
