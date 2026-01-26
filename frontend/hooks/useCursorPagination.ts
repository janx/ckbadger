import { useState, useCallback } from 'react';

export function useCursorPagination() {
  const [cursor, setCursor] = useState<string | undefined>(undefined);
  const [cursorHistory, setCursorHistory] = useState<string[]>([]);

  const goToNext = useCallback(
    (nextCursor: string | null | undefined) => {
      if (nextCursor) {
        setCursorHistory((prev) => [...prev, cursor || '']);
        setCursor(nextCursor);
      }
    },
    [cursor]
  );

  const goToPrevious = useCallback(() => {
    if (cursorHistory.length > 0) {
      const prev = cursorHistory[cursorHistory.length - 1];
      setCursorHistory((h) => h.slice(0, -1));
      setCursor(prev || undefined);
    }
  }, [cursorHistory]);

  const reset = useCallback(() => {
    setCursor(undefined);
    setCursorHistory([]);
  }, []);

  return {
    cursor,
    hasPrevious: cursorHistory.length > 0,
    goToNext,
    goToPrevious,
    reset,
  };
}
