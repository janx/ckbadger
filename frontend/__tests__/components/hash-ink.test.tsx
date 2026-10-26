import { render, screen } from '@/__tests__/utils/test-utils';
import { Hash } from '@/components/ui/hash';
it('renders hashes in neutral ink instead of a data hue', () => {
  render(<Hash hash="0xabcd" copyable={false} />);
  expect(screen.getByText('0xabcd')).toHaveClass('text-text');
  expect(screen.getByText('0xabcd')).not.toHaveClass('text-aqua-dim');
});
