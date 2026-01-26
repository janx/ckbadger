import { renderHook, act } from '@testing-library/react';
import { useCursorPagination } from '@/hooks/useCursorPagination';

describe('useCursorPagination', () => {
  it('initializes with undefined cursor and no history', () => {
    const { result } = renderHook(() => useCursorPagination());

    expect(result.current.cursor).toBeUndefined();
    expect(result.current.hasPrevious).toBe(false);
  });

  describe('goToNext', () => {
    it('updates cursor when given a valid next cursor', () => {
      const { result } = renderHook(() => useCursorPagination());

      act(() => {
        result.current.goToNext('cursor-1');
      });

      expect(result.current.cursor).toBe('cursor-1');
    });

    it('does not update cursor when given null', () => {
      const { result } = renderHook(() => useCursorPagination());

      act(() => {
        result.current.goToNext('cursor-1');
      });

      act(() => {
        result.current.goToNext(null);
      });

      expect(result.current.cursor).toBe('cursor-1');
    });

    it('does not update cursor when given undefined', () => {
      const { result } = renderHook(() => useCursorPagination());

      act(() => {
        result.current.goToNext('cursor-1');
      });

      act(() => {
        result.current.goToNext(undefined);
      });

      expect(result.current.cursor).toBe('cursor-1');
    });

    it('stores previous cursor in history', () => {
      const { result } = renderHook(() => useCursorPagination());

      act(() => {
        result.current.goToNext('cursor-1');
      });

      expect(result.current.hasPrevious).toBe(true);
    });
  });

  describe('goToPrevious', () => {
    it('restores previous cursor from history', () => {
      const { result } = renderHook(() => useCursorPagination());

      act(() => {
        result.current.goToNext('cursor-1');
      });
      act(() => {
        result.current.goToNext('cursor-2');
      });

      expect(result.current.cursor).toBe('cursor-2');

      act(() => {
        result.current.goToPrevious();
      });

      expect(result.current.cursor).toBe('cursor-1');
    });

    it('restores to undefined when going back to first page', () => {
      const { result } = renderHook(() => useCursorPagination());

      act(() => {
        result.current.goToNext('cursor-1');
      });

      act(() => {
        result.current.goToPrevious();
      });

      expect(result.current.cursor).toBeUndefined();
    });

    it('does nothing when already on first page', () => {
      const { result } = renderHook(() => useCursorPagination());

      act(() => {
        result.current.goToPrevious();
      });

      expect(result.current.cursor).toBeUndefined();
      expect(result.current.hasPrevious).toBe(false);
    });

    it('updates hasPrevious correctly', () => {
      const { result } = renderHook(() => useCursorPagination());

      act(() => {
        result.current.goToNext('cursor-1');
      });
      act(() => {
        result.current.goToNext('cursor-2');
      });

      expect(result.current.hasPrevious).toBe(true);

      act(() => {
        result.current.goToPrevious();
      });

      expect(result.current.hasPrevious).toBe(true);

      act(() => {
        result.current.goToPrevious();
      });

      expect(result.current.hasPrevious).toBe(false);
    });
  });

  describe('reset', () => {
    it('resets cursor to undefined', () => {
      const { result } = renderHook(() => useCursorPagination());

      act(() => {
        result.current.goToNext('cursor-1');
      });
      act(() => {
        result.current.goToNext('cursor-2');
      });

      act(() => {
        result.current.reset();
      });

      expect(result.current.cursor).toBeUndefined();
    });

    it('clears history', () => {
      const { result } = renderHook(() => useCursorPagination());

      act(() => {
        result.current.goToNext('cursor-1');
      });
      act(() => {
        result.current.goToNext('cursor-2');
      });

      act(() => {
        result.current.reset();
      });

      expect(result.current.hasPrevious).toBe(false);
    });
  });

  describe('navigation sequence', () => {
    it('handles complex navigation correctly', () => {
      const { result } = renderHook(() => useCursorPagination());

      act(() => result.current.goToNext('page2'));
      act(() => result.current.goToNext('page3'));
      act(() => result.current.goToNext('page4'));

      expect(result.current.cursor).toBe('page4');

      act(() => result.current.goToPrevious());
      expect(result.current.cursor).toBe('page3');

      act(() => result.current.goToNext('page4-new'));
      expect(result.current.cursor).toBe('page4-new');

      act(() => result.current.goToPrevious());
      expect(result.current.cursor).toBe('page3');

      act(() => result.current.goToPrevious());
      expect(result.current.cursor).toBe('page2');

      act(() => result.current.goToPrevious());
      expect(result.current.cursor).toBeUndefined();
    });
  });
});
