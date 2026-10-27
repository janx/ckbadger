import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';
import { render } from '../utils/test-utils';
import { DetailLayout } from '@/components/ui/detail-layout';

describe('DetailLayout identity', () => {
  it('keeps prose words intact while allowing an unbroken identifier to wrap', () => {
    const identity = `Reachability, advertisements and local peer sessions are distinct evidence. 0x${'a'.repeat(64)}`;
    render(
      <DetailLayout title="Network" kind="topic" identity={identity}>
        <p>Network evidence</p>
      </DetailLayout>
    );

    const description = screen.getByText(identity);
    expect(description).toHaveTextContent(identity);
    expect(description).toHaveClass('min-w-0', '[overflow-wrap:anywhere]');
    expect(description).not.toHaveClass('break-all');
  });
});
