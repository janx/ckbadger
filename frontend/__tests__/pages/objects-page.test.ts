import { describe, it, expect, vi } from 'vitest';
import { redirect } from '@/src/navigation';
import ObjectsPage from '@/app/objects/page';

vi.mock('@/src/navigation', () => ({
  redirect: vi.fn(),
}));

describe('ObjectsPage', () => {
  it('redirects to assets object tab', () => {
    ObjectsPage();
    expect(redirect).toHaveBeenCalledWith('/assets?type=object');
  });
});
