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

  // Lane c2-finish, item 1e: a quick double click on `next →` while the page keeps its previous
  // data asks for the same next cursor twice; the walk takes it once, so "page 3" never names
  // page 2's rows.
  describe('the same cursor twice', () => {
    it('takes a next cursor it already stands on once, in one act or in two', () => {
      const { result } = renderHook(() => useCursorPagination());
      act(() => result.current.goToNext('c1'));
      act(() => result.current.goToNext('c1'));
      expect(result.current.page).toBe(2);
      act(() => {
        result.current.goToNext('c2');
        result.current.goToNext('c2');
      });
      expect(result.current.page).toBe(3);
      act(() => result.current.goToPrevious());
      expect(result.current.cursor).toBe('c1');
      act(() => result.current.goToPrevious());
      expect(result.current.cursor).toBeUndefined();
      expect(result.current.page).toBe(1);
    });
  });

  // Item 1d: a list that asks for another page size below md (24 / 20, 50 / 20) starts again from
  // its first page when the size changes: the cursors of the other size name other rows.
  describe('a change of page size', () => {
    it('starts the list again from its first page', () => {
      const { result, rerender } = renderHook(({ size }) => useCursorPagination(size), {
        initialProps: { size: 50 },
      });
      act(() => result.current.goToNext('c1'));
      act(() => result.current.goToNext('c2'));
      expect(result.current.page).toBe(3);
      rerender({ size: 20 });
      expect(result.current.page).toBe(1);
      expect(result.current.cursor).toBeUndefined();
      expect(result.current.hasPrevious).toBe(false);
      // the same size keeps the walk
      act(() => result.current.goToNext('d1'));
      rerender({ size: 20 });
      expect(result.current.page).toBe(2);
    });
  });
});
