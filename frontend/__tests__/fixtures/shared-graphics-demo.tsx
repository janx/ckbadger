// Storybook-free demo of the F1-c shared graphics with board-shaped data (no route): the
// shared-graphics test renders it at 1440 and 390 px.
import { AlignedBar, AlignedBarFootnote } from '@/components/ui/aligned-bar';
import { IciclePartition } from '@/components/ui/icicle-partition';
import { ConcentrationBar } from '@/components/ui/concentration-bar';
import { TenureBar } from '@/components/ui/tenure-bar';

const DAY = 86_400_000;
const REG = Date.parse('2024-02-20T00:00:00Z');

export function SharedGraphicsDemo({ width }: { width: number }) {
  return (
    <div className="min-w-0 space-y-10 overflow-hidden" style={{ width: `${width}px` }}>
      <div className="space-y-2">
        {[
          [652, 1_120],
          [640, 0],
          [671, 2_905],
        ].map(([overhead, user], i) => (
          <AlignedBar
            key={i}
            label="Size, bytes"
            scale={4000}
            segments={[
              { key: 'overhead', label: 'block overhead', value: overhead, tone: 'overhead' },
              { key: 'user', label: 'user transactions', value: user, tone: 'data' },
            ]}
          />
        ))}
        <AlignedBarFootnote scale="0–4,000 B" cap="the consensus limit is 597,000 B" />
      </div>
      <IciclePartition
        label="Secondary issuance since genesis, split three ways"
        formatValue={(v) => `${v.toLocaleString('en-US')} shannons`}
        root={{
          key: 'total',
          label: 'Secondary issuance since genesis',
          value: BigInt('931471350667681431'),
          tone: 'ckb',
          children: [
            {
              key: 'miners',
              label: 'to miners',
              value: BigInt('96313937583612266'),
              tone: 'ckb',
              placeholder: 'paid in cellbases',
            },
            {
              key: 'comp',
              label: 'compensation for depositors',
              value: BigInt('159931858003270345'),
              tone: 'dao',
              children: [
                { key: 'paid', label: 'paid out', value: BigInt('76085000000000000'), tone: 'dao' },
                {
                  key: 'owed',
                  label: 'still owed',
                  value: BigInt('83846858003270345'),
                  tone: 'dao',
                  opacity: 0.4,
                },
              ],
            },
            {
              key: 'burnt',
              label: 'burnt',
              value: BigInt('675225555080798820'),
              tone: 'other',
              placeholder: 'No cell can ever claim it.',
            },
          ],
        }}
      />
      <ConcentrationBar
        label="Share of deposits"
        assetClass="dao"
        total={BigInt(10_000)}
        groups={[
          { label: 'largest', value: BigInt(827) },
          { label: 'next 9', value: BigInt(1_793) },
          { label: 'next 90', value: BigInt(3_107) },
          { label: 'the other 16,541', value: BigInt(4_273) },
        ]}
      />
      <TenureBar
        name="valar.bit"
        periods={[
          { label: 'paid at registration', start: REG, end: REG + 365 * DAY },
          { label: 'renewed 2025-02-04', start: REG + 365 * DAY, end: REG + 730 * DAY },
          { label: 'renewed 2025-02-18', start: REG + 730 * DAY, end: REG + 1095 * DAY },
        ]}
        now={REG + 979 * DAY}
      />
      <TenureBar
        name="apt.cell"
        periods={[{ label: 'registered, one year', start: REG, end: REG + 365 * DAY }]}
        graceEnd={REG + 395 * DAY}
        now={REG + 100 * DAY}
      />
    </div>
  );
}
