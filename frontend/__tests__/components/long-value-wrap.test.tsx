// Responsive fix plan 2026-11-01, lane A (gate finding): a full address or a hash inside running
// text has no break opportunity, so it set the page width — 76 px past 640 on a token page, 326 px
// past 390, and 154 px past 390 on a cluster whose decoder reason names a hash. Long values wrap
// inside their own box (docs/RESPONSIVE_LAYOUT.md, principle 1).
import { render, screen } from '@/__tests__/utils/test-utils';
import { CurrentPatternEvidence } from '@/components/asset/genetics';
import { objectGeneticsProducer } from '@/__tests__/msw/fixtures/item-producer';
import type { ObjectGenetics } from '@/lib/api';

it('wraps a decoder reason that names a hash inside its box', () => {
  const data: ObjectGenetics = structuredClone(objectGeneticsProducer);
  if (data.currentPattern.status !== 'dob') throw new Error('fixture');
  const reason = `byte mapping is not defined for binary 0x${'da35'.repeat(16)}`;
  data.currentPattern.steps[0].description = { status: 'unsupported', reason };
  render(<CurrentPatternEvidence pattern={data.currentPattern} />);
  expect(screen.getByRole('status')).toHaveTextContent(reason);
  expect(screen.getByRole('status')).toHaveClass('[overflow-wrap:anywhere]');
});
