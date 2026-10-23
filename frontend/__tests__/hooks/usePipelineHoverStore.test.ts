import { beforeEach, describe, expect, it } from 'vitest';
import { act, renderHook } from '@testing-library/react';
import { usePipelineHoverStore } from '@/hooks/usePipelineHoverStore';

describe('usePipelineHoverStore', () => {
  beforeEach(() => {
    usePipelineHoverStore.getState().clear();
  });

  it('starts with nothing hovered or pinned', () => {
    expect(usePipelineHoverStore.getState()).toMatchObject({
      hoverBlock: null,
      hoverTx: null,
      pinned: null,
      windowBlock: null,
    });
  });

  it('sets hover block, tx and window anchor together', () => {
    usePipelineHoverStore.getState().setHover({ block: 120, tx: '0xabc', window: 120 });
    expect(usePipelineHoverStore.getState()).toMatchObject({
      hoverBlock: 120,
      hoverTx: '0xabc',
      windowBlock: 120,
    });
    usePipelineHoverStore.getState().clearHover();
    expect(usePipelineHoverStore.getState()).toMatchObject({
      hoverBlock: null,
      hoverTx: null,
      windowBlock: null,
    });
  });

  it('toggles a pin and clears it', () => {
    const { togglePin, clearPin } = usePipelineHoverStore.getState();
    togglePin('0x1');
    expect(usePipelineHoverStore.getState().pinned).toBe('0x1');
    togglePin('0x2');
    expect(usePipelineHoverStore.getState().pinned).toBe('0x2');
    togglePin('0x2');
    expect(usePipelineHoverStore.getState().pinned).toBeNull();
    togglePin('0x3');
    clearPin();
    expect(usePipelineHoverStore.getState().pinned).toBeNull();
  });

  it('keeps the pin while the hover changes', () => {
    usePipelineHoverStore.getState().togglePin('0xpin');
    usePipelineHoverStore.getState().setHover({ block: 1, tx: '0xother', window: null });
    usePipelineHoverStore.getState().clearHover();
    expect(usePipelineHoverStore.getState().pinned).toBe('0xpin');
  });

  it('re-renders a subscriber only when its slice changes', () => {
    let renders = 0;
    const { result } = renderHook(() => {
      renders++;
      return usePipelineHoverStore((s) => s.hoverBlock === 7);
    });
    const start = renders;
    act(() => usePipelineHoverStore.getState().setHover({ block: 3, tx: null, window: null }));
    expect(renders).toBe(start);
    act(() => usePipelineHoverStore.getState().setHover({ block: 7, tx: null, window: null }));
    expect(result.current).toBe(true);
    expect(renders).toBe(start + 1);
  });
});
