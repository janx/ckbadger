'use client';

import { useMemo, useRef, useState, useEffect, useCallback } from 'react';
import { createPortal } from 'react-dom';
import Link from 'next/link';
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
    borderColor: 'border-slate-600/50',
    bgGradient: 'from-slate-800/50 to-slate-900/50',
    titleColor: 'text-slate-300',
    countColor: 'text-slate-400',
  },
  proposals: {
    borderColor: 'border-amber-600/50',
    bgGradient: 'from-amber-950/30 to-slate-900/50',
    titleColor: 'text-amber-400',
    countColor: 'text-amber-500/80',
  },
  tip: {
    borderColor: 'border-purple-600/50',
    bgGradient: 'from-purple-950/30 to-slate-900/50',
    titleColor: 'text-purple-400',
    countColor: 'text-purple-500/80',
  },
};

const CATEGORY_COLORS: Record<TxCategory, Record<'mempool' | 'proposals' | 'tip', string>> = {
  normal: {
    mempool: 'bg-slate-500/80 hover:bg-slate-400',
    proposals: 'bg-amber-600/80 hover:bg-amber-500',
    tip: 'bg-purple-600/80 hover:bg-purple-500',
  },
  cellbase: {
    mempool: 'bg-emerald-600/80 hover:bg-emerald-500',
    proposals: 'bg-emerald-600/80 hover:bg-emerald-500',
    tip: 'bg-emerald-600/80 hover:bg-emerald-500',
  },
  dao: {
    mempool: 'bg-cyan-600/80 hover:bg-cyan-500',
    proposals: 'bg-cyan-600/80 hover:bg-cyan-500',
    tip: 'bg-cyan-600/80 hover:bg-cyan-500',
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
            className="pointer-events-none fixed z-[9999] -translate-x-1/2 -translate-y-full whitespace-nowrap rounded-lg border border-slate-700/50 bg-slate-900/95 px-3 py-2 text-xs shadow-xl backdrop-blur-sm"
            style={{
              left: tooltipPos.x,
              top: tooltipPos.y - 8,
            }}
          >
            <div className="mb-1 text-[10px] text-slate-500">{CATEGORY_LABELS[item.category]}</div>
            <div className="mb-1 font-mono text-slate-300">{truncateHash(item.id)}</div>
            <div className="space-y-0.5 text-slate-400">
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
            <div className="absolute left-1/2 top-full -translate-x-1/2 border-4 border-transparent border-t-slate-900/95" />
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
          'cursor-pointer hover:border-purple-500/70 hover:shadow-lg hover:shadow-purple-900/20'
      )}
    >
      <div className="mb-2 flex items-center justify-between sm:mb-3">
        <div>
          <h3 className={cn('text-sm font-bold sm:text-base', config.titleColor)}>{title}</h3>
          {subtitle && <div className="text-[10px] text-slate-500 sm:text-xs">{subtitle}</div>}
        </div>
        <div className={cn('text-right text-xs sm:text-sm', config.countColor)}>
          <span className="font-bold text-white">{totalCount.toLocaleString()}</span>
          <span className="ml-1">txs</span>
        </div>
      </div>

      <div ref={containerRef} className="relative flex-1 overflow-hidden rounded-lg bg-black/20">
        {items.length === 0 ? (
          <div className="flex h-full w-full items-center justify-center text-xs text-slate-600">
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
