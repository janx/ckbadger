'use client';

import { useState, useMemo, memo } from 'react';
import type { IdShortening } from '@/components/ui/hex-code';
import { ELLIPSIS, shortenHash } from '@/lib/shorten';
import { cn } from '@/lib/utils';

interface HexCharProps {
  char: string;
  index: number;
  groupIndex: number;
  isHovered: boolean;
  onHover: (index: number | null) => void;
  color: 'aqua' | 'lavender' | 'gold' | 'rouge';
  showGroupHighlight: boolean;
}

const HexChar = memo(function HexChar({
  char,
  index,
  groupIndex,
  isHovered,
  onHover,
  color,
  showGroupHighlight,
}: HexCharProps) {
  const colorClasses = {
    aqua: {
      base: 'text-aqua-dim',
      hover: 'text-aqua',
    },
    lavender: {
      base: 'text-lavender-dim',
      hover: 'text-lavender',
    },
    gold: {
      base: 'text-gold-dim',
      hover: 'text-gold',
    },
    rouge: {
      base: 'text-rouge-dim',
      hover: 'text-rouge',
    },
  };

  const groupColorKeys: Array<'aqua' | 'lavender' | 'gold' | 'rouge'> = [
    'aqua',
    'lavender',
    'gold',
    'rouge',
  ];
  const effectiveColor = showGroupHighlight ? groupColorKeys[groupIndex % 4] : color;

  return (
    <span
      className={cn(
        'hex-char inline-block cursor-default transition-all duration-100',
        'font-mono tabular-nums',
        isHovered ? colorClasses[effectiveColor].hover : colorClasses[effectiveColor].base
      )}
      style={{
        textShadow: isHovered ? '0 0 6px currentColor' : 'none',
        transform: isHovered ? 'scale(1.05)' : 'scale(1)',
      }}
      onMouseEnter={() => onHover(index)}
      onMouseLeave={() => onHover(null)}
    >
      {char}
    </span>
  );
});

type HexDisplayProps = IdShortening & {
  value: string;
  groupSize?: number;
  showGroupHighlight?: boolean;
  copyable?: boolean;
  color?: 'aqua' | 'lavender' | 'gold' | 'rouge';
  /** The type step: `sm` is the small step, `md` and `lg` the body step. */
  textSize?: 'sm' | 'md' | 'lg';
  className?: string;
  mono?: boolean;
};

export function HexDisplay({
  value,
  groupSize = 4,
  showGroupHighlight = true,
  copyable = true,
  color = 'aqua',
  textSize = 'md',
  className,
  mono = true,
  ...shortening
}: HexDisplayProps) {
  const [hoveredIndex, setHoveredIndex] = useState<number | null>(null);
  const [copied, setCopied] = useState(false);
  const truncate = shortening.truncate !== false;

  // The value at its size (U-09), the `0x` drawn apart in the prefix ink.
  const written = shortening.truncate === false ? value : shortenHash(value, shortening.size);
  const prefix = written.startsWith('0x') ? '0x' : '';
  const displayValue = written.slice(prefix.length);

  const chars = useMemo(() => {
    const result: { char: string; groupIndex: number; originalIndex: number }[] = [];
    let groupIndex = 0;
    let charInGroup = 0;

    for (let i = 0; i < displayValue.length; i++) {
      const char = displayValue[i];

      if (char === ELLIPSIS) {
        result.push({ char, groupIndex: -1, originalIndex: i });
        continue;
      }

      result.push({ char, groupIndex, originalIndex: i });
      charInGroup++;

      if (charInGroup >= groupSize) {
        charInGroup = 0;
        groupIndex++;
      }
    }

    return result;
  }, [displayValue, groupSize]);

  const handleCopy = async () => {
    if (!copyable) return;
    try {
      await navigator.clipboard.writeText(value);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* clipboard unavailable */
    }
  };

  const sizeClasses = {
    sm: 'text-small',
    md: 'text-body',
    lg: 'text-body',
  };

  const prefixColorClasses = {
    aqua: 'text-aqua-dim',
    lavender: 'text-lavender-dim',
    gold: 'text-gold-dim',
    rouge: 'text-rouge-dim',
  };

  const allowWrap = !truncate;

  if (copied) {
    return (
      <span
        className={cn(
          'inline-flex items-center gap-1',
          allowWrap && 'max-w-full flex-wrap break-all',
          mono && 'font-mono',
          sizeClasses[textSize],
          className
        )}
      >
        <span className="text-jade animate-confirm-fade">Copied</span>
      </span>
    );
  }

  return (
    <span
      className={cn(
        'inline-flex items-center',
        allowWrap && 'max-w-full flex-wrap break-all',
        mono && 'font-mono',
        sizeClasses[textSize],
        copyable && 'cursor-pointer hover:opacity-90',
        className
      )}
      onClick={handleCopy}
      title={copyable ? `Click to copy: ${value}` : value}
    >
      {prefix && <span className={cn('mr-0.5', prefixColorClasses[color])}>{prefix}</span>}

      {chars.map(({ char, groupIndex, originalIndex }) => {
        if (char === ELLIPSIS) {
          return (
            <span key={`ellipsis-${originalIndex}`} className="text-text-dim mx-0.5">
              {char}
            </span>
          );
        }

        return (
          <HexChar
            key={`char-${originalIndex}`}
            char={char}
            index={originalIndex}
            groupIndex={groupIndex}
            isHovered={hoveredIndex === originalIndex}
            onHover={setHoveredIndex}
            color={color}
            showGroupHighlight={showGroupHighlight}
          />
        );
      })}
    </span>
  );
}
