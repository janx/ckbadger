// Readability 2026-11-18, lane B4 (pG, R9), item 1: a shortened address is `ckb1…` and its tail,
// so the lock beside it is named in plain words at the row's own size, and only when it is not the
// default lock (no name means the default lock). The `.small-caps` tag goes: at 12 px its capitals
// were under 10 px, and "DEFAULT LOCK" stood on every line of every feed.
import { describe, expect, it } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { ActivityRow, type ActivityRowData } from '@/components/activity/activity-row';
import { LockKind } from '@/components/ui/lock-kind';
import type { ParticipantInfo } from '@/lib/api';
import { DEFAULT_LOCK_CODE_HASH } from '@/lib/lock-script';
import { activityRead } from '@/__tests__/utils/activity-read';

// Real addresses (RFC 0021 full format): the default lock, and JoyID, over the same args.
const DEFAULT_ADDRESS =
  'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqdnnw7qkdnnclfkg59uzn8umtfd2kwxceqxwquc4';
const JOYID_ADDRESS =
  'ckb1qrgqep8saj8agswr30pls73hra28ry8jlnlc3ejzh3dl2ju7xxpjxqgqqxeeh0qtxeeu05my2z7pfn7d45k4t8rvvsfavygk';
const JOYID_CODE = '0xd00c84f0ec8fd441c38bc3f87a371f547190f2fcff88e642bc5bf54b9e318323';

function party(address: string, lockKind: string | null, lockHash: string): ParticipantInfo {
  return {
    address,
    lockHash,
    lockHashPrefix: null,
    lockKind,
    roles: [],
    tags: 0,
    ckbDelta: '-100000000',
    usedDelta: '0',
    itemDeltas: [],
  };
}

const row = (participants: ParticipantInfo[]): ActivityRowData =>
  activityRead({
    txHash: `0x${'ab'.repeat(32)}`,
    blockNumber: 21000000,
    timestamp: String(Date.now() - 60_000),
    isCellbase: false,
    protocolActions: [],
    typeCalls: [],
    lockCalls: [],
    itemTypes: [],
    itemTypesUnread: null,
    participants,
  });

const lines = (c: HTMLElement) => [...c.querySelectorAll<HTMLElement>('[data-role="participant"]')];

describe('the participant line names a lock only when it is not the default lock (B4 item 1)', () => {
  it('writes no lock name beside a default-lock address', () => {
    const { container } = render(
      <ActivityRow
        activity={row([party(DEFAULT_ADDRESS, 'Default Lock', `0x${'01'.repeat(32)}`)])}
      />
    );
    const [line] = lines(container);
    expect(line.querySelector('[data-role="lock-kind"]')).toBeNull();
    expect(line).not.toHaveTextContent('Default Lock');
  });

  it('names any other lock in plain words at the row’s own size, quiet', () => {
    const { container } = render(
      <ActivityRow activity={row([party(JOYID_ADDRESS, 'JoyID', `0x${'02'.repeat(32)}`)])} />
    );
    const [line] = lines(container);
    const kind = line.querySelector('[data-role="lock-kind"] [data-lock-kind]') as HTMLElement;
    expect(kind).toHaveTextContent(/^JoyID$/);
    // the code hash it was read off, in its hover
    expect(kind).toHaveAttribute('title', JOYID_CODE);
    // plain words: no small capitals, no case change, no size of its own (the line's 13 px)
    expect(kind).not.toHaveClass('small-caps');
    expect(kind).not.toHaveClass('lowercase');
    expect(kind.className).not.toMatch(/\btext-(meta|small|body)\b/);
    expect(kind).toHaveClass('text-text-dim');
  });

  it('names a lock the registry does not name by its code hash (U-15), as LockKind does', () => {
    const { container } = render(
      <ActivityRow activity={row([party(JOYID_ADDRESS, null, `0x${'03'.repeat(32)}`)])} />
    );
    const [line] = lines(container);
    const kind = line.querySelector('[data-role="lock-kind"]') as HTMLElement;
    expect(kind).toHaveTextContent(/^lock 0xd00c84f0…318323$/);
  });
});

describe('a party written by its lock hash (no address) keeps the name the API sends', () => {
  it('names its lock in the same words, with no code hash to hover', () => {
    const noAddress = { ...party(JOYID_ADDRESS, 'Long Args Lock', `0x${'04'.repeat(32)}`) };
    const { container } = render(<ActivityRow activity={row([{ ...noAddress, address: null }])} />);
    const kind = lines(container)[0].querySelector(
      '[data-role="lock-kind"] [data-lock-kind]'
    ) as HTMLElement;
    expect(kind).toHaveTextContent(/^Long Args Lock$/);
    expect(kind).toHaveClass('text-text-dim');
    expect(kind).not.toHaveAttribute('title');
  });

  it('writes nothing when neither its address nor its lock is known', () => {
    const { container } = render(
      <ActivityRow
        activity={row([{ ...party(JOYID_ADDRESS, null, `0x${'05'.repeat(32)}`), address: null }])}
      />
    );
    expect(lines(container)[0].querySelector('[data-role="lock-kind"]')).toBeNull();
  });
});

describe('LockKind: plain words, and nothing for the default lock (B4 item 1)', () => {
  it('draws a name in plain words by default: no small capitals, no case change', () => {
    render(<LockKind address={null} size="row" lockKind="JoyID" lockCodeHash={JOYID_CODE} />);
    const kind = screen.getByText('JoyID');
    expect(kind).not.toHaveClass('small-caps');
    expect(kind).not.toHaveClass('lowercase');
    expect(kind).toHaveClass('text-text-dim');
  });

  // B4 fix round (re-pinned on purpose): the default lock is the pair, read off the row's address
  it('writes nothing for the default lock, whatever the form', () => {
    const { container } = render(
      <LockKind
        address={DEFAULT_ADDRESS}
        size="row"
        lockKind="Default Lock"
        lockCodeHash={DEFAULT_LOCK_CODE_HASH}
      />
    );
    expect(container).toBeEmptyDOMElement();
  });
});
