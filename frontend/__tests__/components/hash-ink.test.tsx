import { render, screen } from '@/__tests__/utils/test-utils';
import { Hash } from '@/components/ui/hash';
it('renders hashes in neutral ink instead of a data hue', () => {
  render(<Hash hash="0xabcd" size="row" />);
  expect(screen.getByText('0xabcd')).toHaveClass('text-text');
  expect(screen.getByText('0xabcd')).not.toHaveClass('text-aqua-dim');
});

// Lists lane P3 (the link-look gate reads a child's ink inside a link): a hash that is a row's key
// stands inside a link of the key look, whose bright ink is the look's; the hash writes no ink of
// its own there (its default reading ink beat the link's look, so a key read in the reading ink).
it('writes no ink of its own where it inherits its link’s look', () => {
  render(
    <a href="/tx/0xabcd" className="text-text-bright">
      <Hash hash="0xabcd" size="row" variant="inherit" />
    </a>
  );
  const shown = screen.getByText('0xabcd');
  expect(shown).not.toHaveClass('text-text');
  expect(shown).not.toHaveClass('text-text-bright');
  expect(shown.closest('a')).toHaveClass('text-text-bright');
});
