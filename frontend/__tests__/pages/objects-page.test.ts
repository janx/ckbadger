import { describe, it, expect, vi } from 'vitest';
import { redirect } from '@/src/navigation';
import ObjectsPage from '@/app/objects/page';

vi.mock('@/src/navigation', () => ({
  redirect: vi.fn(),
}));

describe('ObjectsPage', () => {
  it('redirects to inventory objects page', () => {
    ObjectsPage();
    expect(redirect).toHaveBeenCalledWith('/inventory/objects');
  });
});
