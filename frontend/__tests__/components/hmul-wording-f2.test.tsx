// F2 group `objects` (IV-07): the H-multiplier is explained in the board's words — CKB held ÷
// occupied — not as "Owned Capacity / Common Knowledge".
import { render, screen, fireEvent } from '@/__tests__/utils/test-utils';
import { HMulHelpPopover } from '@/components/ui/hmul-info';

it('explains HMul as CKB held over occupied', () => {
  render(<HMulHelpPopover />);
  fireEvent.click(screen.getByRole('button', { name: 'Explain HMul' }));
  const text = document.body.textContent ?? '';
  expect(text).toContain('CKB held in its cells ÷ occupied');
  expect(text).not.toContain('Common Knowledge');
});
