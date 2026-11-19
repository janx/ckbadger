import { afterEach, describe, expect, it, vi } from 'vitest';
import { render } from '@/__tests__/utils/test-utils';
import { atViewportWidth } from '@/__tests__/utils/viewport';
import { seededAddress } from '@/__tests__/utils/ckb-address';
import { ActivityRow, type ActivityRowData } from '@/components/activity/activity-row';
import type { ParticipantInfo } from '@/lib/api';
import { PHONE_LINE_PX } from '@/lib/type-metrics';
import { activityRead } from '@/__tests__/utils/activity-read';

// Lane D4 (the phone-rows board, its activities pane; PA-10): an activity row took three shapes on a
// phone by the length of its strings — the block and its age dropped under the headline when the
// headline and what was called ran long, each party's CKB followed its address with no common right
// edge, and a long amount went to a line of its own. Below sm the row is the board's: the headline
// and, on its line, the block and the age (the headline wraps between its words beside them); what
// was called on a line of its own; each party one line, its CKB on the row's one right edge; what
// else the party moved and its occupied bytes on the line under it, the bytes quiet at the right.
// Only the layout: the words, the fold and the parties' rules are gate B's and stay as they are.

function party(seed: string, ckbDelta: string, usedDelta = '0'): ParticipantInfo {
  return {
    address: seededAddress(seed),
    lockHash: `0x${seed.padStart(64, '0')}`,
    lockHashPrefix: null,
    lockKind: 'Default Lock',
    roles: [],
    tags: 0,
    ckbDelta,
    usedDelta,
    itemDeltas: [],
    sent: false,
  };
}

function row(
  participants: ParticipantInfo[],
  over: Partial<ActivityRowData> = {}
): ActivityRowData {
  return activityRead({
    txHash: `0x${'fe'.repeat(32)}`,
    blockNumber: 21009129,
    timestamp: String(Date.now() - 50_000),
    isCellbase: false,
    protocolActions: [],
    typeCalls: [],
    lockCalls: [],
    recognizedTypes: [],
    participants,
    ...over,
  });
}

const transfer = row([
  party('dcratvu9', '-58171000000', '-6100000000'),
  party('cg8z4zde', '58166000000', '6100000000'),
]);
const scriptCall = row([party('hqqsv05e', '-30000')], {
  typeCalls: [
    {
      typeCodeHash: `0x${'aa'.repeat(32)}`,
      typeHashType: 'type',
      typeArgs: '0x00',
      scriptHash: `0x${'ab'.repeat(32)}`,
      scriptName: '.bit Time Index State',
    },
    {
      typeCodeHash: `0x${'ac'.repeat(32)}`,
      typeHashType: 'type',
      typeArgs: '0x00',
      scriptHash: `0x${'ad'.repeat(32)}`,
      scriptName: '.bit Time Info',
    },
  ],
});

describe('the activity row on a phone (lane D4, PA-10)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('keeps the block and the age on the headline’s line and gives what was called its own', () => {
    atViewportWidth(390);
    const { container } = render(<ActivityRow activity={scriptCall} age />);
    const head = container.querySelector('[data-role="row-head"]')!;
    // one line that does not wrap its parts: the headline takes the room the evidence leaves it
    expect(head).toHaveClass('flex-nowrap');
    expect(head.querySelector('[data-role="headline"]')).toHaveTextContent('Script call');
    expect(head.querySelector('[data-role="evidence"]')).toHaveTextContent(
      /^#21,009,129 · \d+s ago$/
    );
    // what was called is the next line, not a part of the headline's list
    expect(head).not.toHaveTextContent('.bit Time');
    const called = container.querySelector('[data-role="head-details"]')!;
    expect(called).toHaveTextContent('.bit Time Index State · .bit Time Info');
    expect(head.compareDocumentPosition(called) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('writes each party on one line, its CKB on the row’s one right edge, the bytes under it', () => {
    atViewportWidth(390);
    const { container } = render(<ActivityRow activity={transfer} age />);
    const parties = Array.from(container.querySelectorAll('[data-role="participant"]'));
    expect(parties).toHaveLength(2);
    for (const p of parties) {
      const line = p.querySelector(':scope > [data-role="party-line"]')!;
      // the address and its lock wrap beside the amount; the amount never leaves the line's end
      expect(line).toHaveClass('flex-nowrap');
      const ckb = line.querySelector(':scope > [data-role="ckb"]')!;
      expect(ckb).toBe(line.lastElementChild);
      expect(ckb).toHaveClass('shrink-0', 'ml-auto');
      // the occupied bytes close the party in the quiet ink, on the line under it, at the right
      const figures = p.querySelector(':scope > [data-role="party-figures"]')!;
      expect(figures.querySelector('[data-role="occupied"]')).toHaveClass(
        'text-text-dim',
        'ml-auto'
      );
      expect(line.querySelector('[data-role="occupied"]')).toBeNull();
    }
    expect(parties[0].querySelector('[data-role="ckb"]')).toHaveTextContent('−581.71 CKB');
    expect(parties[1].querySelector('[data-role="ckb"]')).toHaveTextContent('+581.66 CKB');
    expect(parties[0].querySelector('[data-role="occupied"]')).toHaveTextContent('occupied −61 B');
  });

  // The board's head(): the mark and its 8 px, 12 characters of headline at 14 px, the 12 px and the
  // widest evidence at 12 px, "#99,999,999 · 10y 364d ago": 324 of 328 px at 360.
  it('states its head line at 360 px, 324 of 328, the widest evidence beside 12 characters', () => {
    atViewportWidth(360);
    const { container } = render(<ActivityRow activity={scriptCall} age />);
    const head = container.querySelector('[data-role="row-head"]')!;
    expect(head).toHaveAttribute('data-head-px', '324');
    expect(Number(head.getAttribute('data-head-px'))).toBeLessThanOrEqual(PHONE_LINE_PX[360]);
  });

  it('keeps the desktop row as it is from sm', () => {
    atViewportWidth(1024);
    const { container } = render(<ActivityRow activity={scriptCall} age />);
    const head = container.querySelector('[data-role="row-head"]')!;
    expect(head).toHaveClass('flex-wrap');
    expect(head).toHaveTextContent('.bit Time Index State');
    expect(container.querySelector('[data-role="head-details"]')).toBeNull();
    expect(container.querySelector('[data-role="party-line"]')).toBeNull();
  });
});
