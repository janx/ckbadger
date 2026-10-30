import { render, screen } from '@/__tests__/utils/test-utils';
import { activityHeadline, protocolActionLabel } from '@/components/activity-event-row';
import { ActivityRow } from '@/components/activity/activity-row';
import { feedSamples } from '@/__tests__/msw/fixtures/home';
import type { GlobalActivity } from '@/lib/api';

it('distinguishes an exact object mint and burn from a transfer', () => {
  const activity = feedSamples.objectTransfer;
  const receiver = activity.participants.find((p) =>
    p.itemDeltas.some((d) => d.kind === 'object' && d.delta > 0)
  )!;
  const sender = activity.participants.find((p) =>
    p.itemDeltas.some((d) => d.kind === 'object' && d.delta < 0)
  )!;
  expect(activityHeadline({ ...activity, participants: [receiver] })).toBe('Object mint');
  expect(activityHeadline({ ...activity, participants: [sender] })).toBe('Object burn');
  expect(activityHeadline(activity)).toBe('Object transfer');
});
it('does not reinterpret a receiver-only display as a mint', () => {
  const activity = feedSamples.objectTransfer;
  const participant = activity.participants.find((p) =>
    p.itemDeltas.some((d) => d.kind === 'object' && d.delta > 0)
  )!;
  render(<ActivityRow activity={activity} mode="participant" participant={participant} />);
  expect(screen.getByText('Object transfer')).toBeInTheDocument();
});
it('uses arbitrary-precision token net movement for supply-changing actions', () => {
  const activity: GlobalActivity = {
    ...feedSamples.tokenTransfer,
    participants: feedSamples.tokenTransfer.participants.map((p, i) => ({
      ...p,
      itemDeltas: [
        {
          kind: 'token',
          typeScriptHash: '0xtoken',
          delta: i === 0 ? '-900719925474099200000' : '900719925474099200001',
          symbol: 'TKN',
          decimals: 8,
        },
      ],
    })),
  };
  expect(activityHeadline(activity)).toBe('Token mint');
});

it.each([
  ['leap_to_btc', 'Leap to Bitcoin'],
  ['btc_time_locked', 'Bitcoin time lock'],
  ['leap_to_ckb', 'Leap to CKB'],
  ['transfer', 'Transfer'],
])('names RGB++ %s in plain language', (action, label) => {
  expect(
    protocolActionLabel({ protocol: 'rgbpp', protocolName: 'RGB++', action, metadata: {} })
  ).toBe(`RGB++ · ${label}`);
});
it('shows exact RGB++ Bitcoin evidence with the lock transition', () => {
  const activity = {
    ...feedSamples.tokenTransfer,
    protocolActions: [
      {
        protocol: 'rgbpp',
        protocolName: 'RGB++',
        action: 'leap_to_btc',
        metadata: { btcTxid: `0x${'a'.repeat(64)}`, outIndex: '2' },
      },
    ],
  };
  render(<ActivityRow activity={activity} />);
  expect(screen.getByText('RGB++ · Leap to Bitcoin')).toBeInTheDocument();
  expect(screen.getByText('CKB lock → Bitcoin-controlled lock')).toBeInTheDocument();
  expect(screen.getByTitle(`0x${'a'.repeat(64)}`)).toHaveTextContent(':2');
});

it('interprets an uncommitted body without fabricated block evidence', () => {
  const { participants, protocolActions, typeCalls, lockCalls, isCellbase } =
    feedSamples.objectTransfer;
  expect(
    activityHeadline({ participants, protocolActions, typeCalls, lockCalls, isCellbase })
  ).toBe('Object transfer');
});

it('names a .cell ring_relink as the ring pointer it changed, without throwing', () => {
  const ringRelink = {
    protocol: 'dotcell',
    protocolName: '.cell',
    action: 'ring_relink',
    metadata: { label: 'apt', name: 'apt.cell', id: '0x' + 'a7'.repeat(20), changes: ['next'] },
  };
  expect(protocolActionLabel(ringRelink)).toBe('.cell · ring pointer changed for apt.cell');
  // A ring_relink names no participants and moves no items; it labels on its own.
  expect(protocolActionLabel({ ...ringRelink, metadata: { changes: ['next'] } })).toBe(
    '.cell · ring pointer changed'
  );
});
it('shows a register beside the ring_relink it caused in one transaction', () => {
  const activity: GlobalActivity = {
    ...feedSamples.tokenTransfer,
    participants: feedSamples.tokenTransfer.participants.map((p) => ({ ...p, itemDeltas: [] })),
    typeCalls: [],
    protocolActions: [
      {
        protocol: 'dotcell',
        protocolName: '.cell',
        action: 'register',
        metadata: { label: 'ckb', name: 'ckb.cell', id: '0x' + 'cc'.repeat(20) },
      },
      {
        protocol: 'dotcell',
        protocolName: '.cell',
        action: 'ring_relink',
        metadata: { label: 'apt', name: 'apt.cell', id: '0x' + 'a7'.repeat(20), changes: ['next'] },
      },
    ],
  };
  render(<ActivityRow activity={activity} />);
  expect(screen.getByText('.cell · register')).toBeInTheDocument();
  expect(screen.getByText(/ring pointer changed for apt\.cell/)).toBeInTheDocument();
  expect(screen.queryByTestId('activity-read-error')).not.toBeInTheDocument();
});
