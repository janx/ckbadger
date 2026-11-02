// Responsive fix plan 2026-11-01 (A3): units that never break inside — a number with its unit, an
// outpoint's ":index", a hyphenated term with its value (docs/RESPONSIVE_LAYOUT.md → Non-breaking
// units; report A#14, B#18, B#22).
import { render, screen } from '@/__tests__/utils/test-utils';
import { CkbDelta } from '@/components/activity-event-row';
import { ItemEvidence } from '@/components/asset/item-evidence';
import { HMultiplierReading } from '@/components/ui/h-multiplier';
import { OutpointIndex } from '@/components/ui/outpoint-index';
import { didProtocolProducer } from '@/__tests__/msw/fixtures/item-producer';
import type { ItemProtocol } from '@/lib/api';

it('keeps a CKB delta and its unit on one line', () => {
  render(<CkbDelta delta="-509135264388627" />);
  const amount = screen.getByText(/CKB$/);
  expect(amount).toHaveTextContent('-5,091,352.64388627 CKB');
  expect(amount).toHaveClass('whitespace-nowrap');
});

it('keeps an outpoint index whole after its hash', () => {
  render(<OutpointIndex index={16} />);
  expect(screen.getByText(':16')).toHaveClass('whitespace-nowrap');
});

it('never splits ":16" from a cell version outpoint in a break-all box (B#22)', () => {
  const protocol: ItemProtocol = structuredClone(didProtocolProducer);
  const shown = protocol.versions[protocol.versions.length - 1].cell;
  shown.outputIndex = 16;
  render(<ItemEvidence protocol={protocol} />);
  const link = screen.getByRole('link', { name: new RegExp(shown.txHash) });
  expect(link.closest('.break-all')).not.toBeNull();
  expect(link.querySelector('[data-outpoint-index]')).toHaveTextContent(/^:16$/);
  expect(link.querySelector('[data-outpoint-index]')).toHaveClass('whitespace-nowrap');
});

it('reads "H-multiplier ×1.02" as one unbroken unit (B#18)', () => {
  render(<HMultiplierReading held={BigInt(102)} occupied={BigInt(100)} />);
  const reading = screen.getByText('H-multiplier ×1.02');
  expect(reading).toHaveAttribute('data-role', 'h-multiplier');
  expect(reading).toHaveClass('whitespace-nowrap');
});
