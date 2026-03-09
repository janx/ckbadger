'use client';

import { useState, useMemo, memo } from 'react';
import { cn } from '@/lib/utils';

interface HexCharProps {
  char: string;
  index: number;
  groupIndex: number;
  isHovered: boolean;
  onHover: (index: number | null) => void;
  color: 'green' | 'amber' | 'white' | 'accent';
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
    green: {
      base: 'text-emphasis-dim',
      hover: 'text-emphasis',
      glow: '0 0 4px rgba(140, 224, 10, 0.6)',
    },
    amber: {
      base: 'text-warning-dim',
      hover: 'text-warning',
      glow: '0 0 4px rgba(255, 176, 0, 0.6)',
    },
    white: {
      base: 'text-text-muted',
      hover: 'text-white',
      glow: '0 0 4px rgba(255, 255, 255, 0.4)',
    },
    accent: {
      base: 'text-emphasis',
      hover: 'text-emphasis',
      glow: '0 0 4px rgba(140, 224, 10, 0.6)',
    },
  };

  const groupColors = ['opacity-100', 'opacity-70', 'opacity-85', 'opacity-55'];
  const groupOpacity = showGroupHighlight ? groupColors[groupIndex % 4] : 'opacity-100';

  return (
    <span
      className={cn(
        'hex-char inline-block cursor-default transition-all duration-100',
        'font-mono tabular-nums',
        isHovered ? colorClasses[color].hover : colorClasses[color].base,
        groupOpacity
      )}
      style={{
        textShadow: isHovered ? colorClasses[color].glow : 'none',
        transform: isHovered ? 'scale(1.05)' : 'scale(1)',
      }}
      onMouseEnter={() => onHover(index)}
      onMouseLeave={() => onHover(null)}
    >
      {char}
    </span>
  );
});

interface HexDisplayProps {
  value: string;
  truncate?: boolean;
  startChars?: number;
  endChars?: number;
  groupSize?: number;
  showGroupHighlight?: boolean;
  copyable?: boolean;
  color?: 'green' | 'amber' | 'white' | 'accent';
  size?: 'sm' | 'md' | 'lg';
  className?: string;
  mono?: boolean;
}

export function HexDisplay({
  value,
  truncate = true,
  startChars = 10,
  endChars = 8,
  groupSize = 4,
  showGroupHighlight = true,
  copyable = true,
  color = 'green',
  size = 'md',
  className,
  mono = true,
}: HexDisplayProps) {
  const [hoveredIndex, setHoveredIndex] = useState<number | null>(null);
  const [copied, setCopied] = useState(false);

  const cleanValue = value.startsWith('0x') ? value.slice(2) : value;
  const prefix = value.startsWith('0x') ? '0x' : '';

  const displayValue = useMemo(() => {
    if (!truncate || cleanValue.length <= startChars + endChars) {
      return cleanValue;
    }
    return `${cleanValue.slice(0, startChars)}...${cleanValue.slice(-endChars)}`;
  }, [cleanValue, truncate, startChars, endChars]);

  const chars = useMemo(() => {
    const result: { char: string; groupIndex: number; originalIndex: number }[] = [];
    let groupIndex = 0;
    let charInGroup = 0;

    for (let i = 0; i < displayValue.length; i++) {
      const char = displayValue[i];

      if (char === '.') {
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
    sm: 'text-xs',
    md: 'text-sm',
    lg: 'text-base',
  };

  const prefixColorClasses = {
    green: 'text-emphasis-dim',
    amber: 'text-warning-dim',
    white: 'text-text-muted',
    accent: 'text-emphasis-dim',
  };

  const allowWrap = !truncate;

  if (copied) {
    return (
      <span
        className={cn(
          'inline-flex items-center gap-1',
          allowWrap && 'max-w-full flex-wrap break-all',
          mono && 'font-mono',
          sizeClasses[size],
          className
        )}
      >
        <span className="text-emphasis animate-subtle-bounce">Copied</span>
      </span>
    );
  }

  return (
    <span
      className={cn(
        'inline-flex items-center',
        allowWrap && 'max-w-full flex-wrap break-all',
        mono && 'font-mono',
        sizeClasses[size],
        copyable && 'cursor-pointer hover:opacity-90',
        className
      )}
      onClick={handleCopy}
      title={copyable ? `Click to copy: ${value}` : value}
    >
      {prefix && <span className={cn('mr-0.5', prefixColorClasses[color])}>{prefix}</span>}

      {chars.map(({ char, groupIndex, originalIndex }) => {
        if (char === '.') {
          return (
            <span key={`ellipsis-${originalIndex}`} className="text-text-muted mx-0.5">
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

interface ByteGroupDisplayProps {
  value: string;
  bytesPerGroup?: number;
  separator?: string;
  color?: 'green' | 'amber' | 'white' | 'accent';
  className?: string;
}

export function ByteGroupDisplay({
  value,
  bytesPerGroup = 4,
  separator = ' ',
  color = 'green',
  className,
}: ByteGroupDisplayProps) {
  const cleanValue = value.startsWith('0x') ? value.slice(2) : value;
  const prefix = value.startsWith('0x') ? '0x' : '';

  const groups = useMemo(() => {
    const charsPerGroup = bytesPerGroup * 2;
    const result: string[] = [];

    for (let i = 0; i < cleanValue.length; i += charsPerGroup) {
      result.push(cleanValue.slice(i, i + charsPerGroup));
    }

    return result;
  }, [cleanValue, bytesPerGroup]);

  const colorClasses = {
    green: ['text-emphasis', 'text-emphasis-dim'],
    amber: ['text-warning', 'text-warning-dim'],
    white: ['text-white', 'text-text-muted'],
    accent: ['text-emphasis', 'text-emphasis-dim'],
  };

  return (
    <span className={cn('font-mono tabular-nums', className)}>
      {prefix && <span className="text-text-muted">{prefix}</span>}
      {groups.map((group, index) => (
        <span key={index}>
          <span className={colorClasses[color][index % 2]}>{group}</span>
          {index < groups.length - 1 && <span className="text-text-muted">{separator}</span>}
        </span>
      ))}
    </span>
  );
}
