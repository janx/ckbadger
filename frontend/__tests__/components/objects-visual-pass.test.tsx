// F2 group `objects` (round 2, visual pass on the live services): what the 1440 / 390 screenshots
// showed against the boards — the history readout defaulting to "No completed weekly total"
// (OB-C6), a gel caption clipped at 16 lanes, tier-2 headings in tier-1 style, "bytes 32–32",
// an m-NFT lock footnote saying "at minting", occupied changes wrapping in the lock-column grid,
// and Newest rows five lines tall at 390.
import { render, screen } from '@/__tests__/utils/test-utils';
import { AssetHistory } from '@/components/asset/asset-history';
import { AlleleGel } from '@/components/ui/allele-gel';
import { ObjectGeneticsEvidence } from '@/components/asset/genetics';
import { MnftPermissions } from '@/components/asset/identity-document';
import { lockFootnote } from '@/components/asset/object-hero';
import { ActivityRow } from '@/components/activity/activity-row';
import { DetailTier2 } from '@/components/ui/detail-layout';
import { alleleLanes, geneOrder } from '@/lib/dob-genes';
import {
  mnftProtocolProducer,
  sporeCellVersionsProducer,
} from '@/__tests__/msw/fixtures/item-producer';
import { nervape3xGenetics, nervape3xInsights } from '@/__tests__/fixtures/objects/nervape3x';

it('reads the latest complete week by default instead of "No completed weekly total" (OB-C6)', () => {
  render(
    <AssetHistory
      daily={[
        { date: '2026-10-27', capacity: '100', occupiedCapacity: '60' },
        { date: '2026-10-29', capacity: '100', occupiedCapacity: '60' },
      ]}
      weekly={[{ weekStart: '2026-10-19', count: 5 }]}
      weeklyNoun="transfers"
      completeThrough="2026-10-29"
    />
  );
  const readout = screen.getByLabelText('History readout');
  expect(readout).toHaveTextContent('Latest complete week, from 2026-10-19: 5 transfers');
  expect(readout).not.toHaveTextContent('No completed weekly total');
});

it('writes the allele gel caption as wrapping text, not clipped SVG text', () => {
  const lanes = alleleLanes(
    nervape3xInsights.genetics,
    geneOrder(nervape3xInsights.currentPattern),
    'Nervape 3x'
  );
  render(<AlleleGel lanes={lanes} total={90} subject="Nervape 3x" />);
  const caption = screen.getByText(/band thickness = share of all 90 objects/);
  expect(caption.closest('svg')).toBeNull();
});

it('styles tier-2 evidence headings as tier 2 and names one byte as "byte N"', () => {
  render(
    <DetailTier2>
      <ObjectGeneticsEvidence evidence={nervape3xGenetics} subject="Nervape 3x" />
      <MnftPermissions
        permissions={mnftProtocolProducer.versions[0].decoded.permissions}
        configure={mnftProtocolProducer.versions[0].decoded.configure}
      />
    </DetailTier2>
  );
  expect(screen.getByRole('heading', { name: 'DNA evidence' })).toHaveClass('text-[13px]');
  expect(screen.getByRole('heading', { name: 'What its configuration permits' })).toHaveClass(
    'text-[13px]'
  );
  const evidence = screen.getByLabelText('DNA evidence');
  expect(evidence).toHaveTextContent('Color byte 32');
  expect(evidence).not.toHaveTextContent('bytes 32–32');
});

it('says "at issue" for an m-NFT token’s lock footnote', () => {
  const v = sporeCellVersionsProducer.data[0];
  const versions = [
    { ...v, lockName: 'UniPass', isLive: false },
    { ...v, lockName: 'PW Lock', lock: { ...v.lock, args: '0x' + '02'.repeat(20) } },
  ];
  expect(lockFootnote(versions, 'token')).toBe(
    'Lock: UniPass with 20-byte args at issue → PW Lock with 20-byte args now. Type and data never changed.'
  );
  expect(lockFootnote(versions, 'object')).toContain('at minting');
});

it('keeps an occupied change on one line in the lock-column grid', () => {
  const { container } = render(
    <ActivityRow
      lockColumn
      activity={{
        txHash: '0x' + 'cd'.repeat(32),
        blockNumber: 1,
        timestamp: '1700000000000',
        isCellbase: false,
        protocolActions: [],
        typeCalls: [],
        lockCalls: [],
        participants: [
          {
            lockHash: '0x' + '01'.repeat(32),
            lockHashPrefix: null,
            address: null,
            roles: [],
            tags: 0,
            lockKind: 'UniPass',
            ckbDelta: '-267999759670',
            usedDelta: '-266000000000',
            itemDeltas: [],
          },
        ],
      }}
    />
  );
  expect(container.querySelector('[data-role="occupied"]')).toHaveClass('whitespace-nowrap');
});
