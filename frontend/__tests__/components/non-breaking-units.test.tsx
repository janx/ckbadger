// Responsive fix plan 2026-11-01 (A3): units that never break inside — a number with its unit, an
// outpoint's ":index", a hyphenated term with its value (docs/RESPONSIVE_LAYOUT.md → Non-breaking
// units; report A#14, B#18, B#22).
import { render, screen } from '@/__tests__/utils/test-utils';
import { ItemEvidence } from '@/components/asset/item-evidence';
import { HMultiplierReading } from '@/components/ui/h-multiplier';
import { OutpointIndex } from '@/components/ui/outpoint-index';
import { didProtocolProducer } from '@/__tests__/msw/fixtures/item-producer';
import type { ItemProtocol } from '@/lib/api';

it('keeps an outpoint index whole after its hash', () => {
  render(<OutpointIndex index={16} />);
  expect(screen.getByText(':16')).toHaveClass('whitespace-nowrap');
});

// B3 round 4 (re-pinned on purpose): the box breaks a long hash with [overflow-wrap:anywhere]
// where it was break-all, which also cut the word "Outpoint" beside it
// Re-pinned on purpose (lane P2's follow-up, item 8; renamed, it was "never splits ":16" from a
// cell version outpoint in a box that breaks the hash (B#22)"): the hash is a head's id (HeadId),
// its lines whole units, never broken anywhere nor scrolled; the index is said in words before it,
// "Outpoint · output 16 of", so no ":16" can leave its hash
it('says a cell version outpoint’s index in words and sets its hash in whole lines (B#22)', () => {
  const protocol: ItemProtocol = structuredClone(didProtocolProducer);
  const shown = protocol.versions[protocol.versions.length - 1].cell;
  shown.outputIndex = 16;
  render(<ItemEvidence protocol={protocol} />);
  const link = screen.getByRole('link', { name: new RegExp(shown.txHash) });
  expect(link.querySelector('[data-head-id]')).toHaveAttribute('title', shown.txHash);
  for (const line of link.querySelectorAll('[data-head-line]'))
    expect(line).toHaveClass('whitespace-nowrap');
  expect(link.closest('div')).toHaveTextContent(/^Outpoint · output 16 of0x/);
});

it('reads "H-multiplier ×1.02" as one unbroken unit (B#18)', () => {
  render(<HMultiplierReading held={BigInt(102)} occupied={BigInt(100)} />);
  const reading = screen.getByText('H-multiplier ×1.02');
  expect(reading).toHaveAttribute('data-role', 'h-multiplier');
  expect(reading).toHaveClass('whitespace-nowrap');
});
